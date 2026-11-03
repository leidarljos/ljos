:description: How the seat meets the rest of the world: the memory systems it learns from, the concurrency designs it voted on, the runners it sits in, and the dogfooding that tests it every day.
:cover: https://leidarljos.github.io/ljos/_static/og.png

How the seat meets the rest of the world: the memory systems it learns
from, the concurrency designs it voted on, the runners it sits in, and
the dogfooding that tests it every day. The :doc:`explanation <explanation>`
says what the seat is; this page says what it plugs into and why.

The seat among FOSS memory systems
==================================

Agent memory systems in the literature and on the market do one of a few
jobs. The seat integrates with them rather than re-running them:

+-----------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+
| System                                                                                                          | What it does                                                                                                                                | How the seat meets it                                                                                                                       |
+=================================================================================================================+=============================================================================================================================================+=============================================================================================================================================+
| Mem0 (doi:10.48550/arXiv.2504.19413)                                                                            | a model extracts facts from a conversation and decides per fact whether to add, update, delete or leave the store.                          | the seat does not extract: it stores an agent's lesson as written, labelled agent-derived and unreviewed, and a review keeps or drops it.   |
|                                                                                                                 |                                                                                                                                             | At ``SessionEnd`` the seat keeps up to five cue-word sentences from the person's and the agent's prose as unverified agent notes the prompt |
|                                                                                                                 |                                                                                                                                             | hook never injects, and ``LJOS_EXTRACT=on`` turns that on. The read is off by default, and a note is written only when the conversation has |
|                                                                                                                 |                                                                                                                                             | an open sitting. With no sitting the hook prints one line and writes nothing, because packset keeps only notes that belong to a ticket.     |
+-----------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+
| Zep (doi:10.48550/arXiv.2501.13956)                                                                             | a temporal knowledge graph with edges carrying the time an edge became true and stopped being true, plus fused lexical, dense and graph     | the pack carries the same two times on every claim (written-at and the live window a superseding claim closes) and fuses a prefix scan,     |
|                                                                                                                 | retrieval                                                                                                                                   | BM25+ and a dense ballot; a Zep graph can be read as trust rows and island links, not as a second store                                     |
+-----------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+
| Letta, formerly MemGPT (doi:10.48550/arXiv.2310.08560), with sleep-time compute (doi:10.48550/arXiv.2504.13171) | a small core memory the model edits with tools, plus recall and archival stores reorganised between turns                                   | the seat's review clock (``ljos due``, ``graded``) is the reorganisation, but it runs on Free Spaced Repetition Scheduler (FSRS)            |
|                                                                                                                 |                                                                                                                                             | retrievability rather than on a model pass, so it works with no model at all                                                                |
+-----------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+
| HippoRAG (doi:10.48550/arXiv.2405.14831, doi:10.48550/arXiv.2502.14802)                                         | a knowledge graph retrieved by personalised PageRank from the entities a question names                                                     | the island (``ljos island``) is the same idea over Hebbian link weights, read beside the fused hits for orientation rather than in their    |
|                                                                                                                 |                                                                                                                                             | place, because measured as a ranking it lost; see the explanation                                                                           |
+-----------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+
| MemoryBank (doi:10.48550/arXiv.2305.10250), Generative Agents (doi:10.48550/arXiv.2304.03442)                   | forgetting on an Ebbinghaus curve refreshed by recall; ranking by recency, importance and relevance plus reflection                         | forgetting here is by review, not by age: FSRS retrievability scales search, and a never-recalled lesson lapses like a missed review        |
+-----------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+
| Seldon (`seldon-code/seldon <https://github.com/seldon-code/seldon>`__)                                         | the Ordinary Differential Equation (ODE) engine for opinion dynamics                                                                        | ``ljos-consensus settle --seldon`` writes its inputs and reads its output, so the discrete step is checked against a second implementation  |
|                                                                                                                 |                                                                                                                                             | without linking its GNU General Public License (GPL) code                                                                                   |
+-----------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------+

The one design choice the others do not make is the privacy of write:
what is remembered is what was said with ``Remember`` or ``Prefer``, stored
as written. Integration never crosses that line: foreign facts arrive as
labelled claims, trust rows, or cited deeds, each attributable and retractable.

Three concurrency designs and the vote
======================================

Three designs were on the table for the seat's concurrent life: many
agents writing at once. The vote went as follows, and the code follows it.

**A Go rewrite for goroutines: no.** The bottleneck is not compute, it is
one writer. The pack is one daemon owning one Lightning Memory-Mapped Database (LMDB) file
(``packsetd``); the claim graph is one in-process graph with the host as
sole mutator (``claimdag``); deed appends take a file lock so two processes
minting at once write whole lines. The packset README records zero
errors at 32 clients, and a herd of 32 writing 6400 distinct claims into
one workspace keeps 6382 live and 18 closed. Goroutines parallelise what is already fast
and leave the single writer single; a rewrite would trade the Rust
type-level guarantees (closed enums for kind, status and role; compare-and-swap on
``generation``) for a runtime that does not change the contention shape.
Concurrency here is checked with a hammer test: ``packsetd`` serves many
readers and one writer, and the hammer rerun is the regression check.

**An Erlang-style plugin tree: no as an Open Telecom Platform (OTP) port, yes as the pattern.**
What Erlang would buy is supervision: a crashed worker restarts, and a bad
message is refused at the boundary. The seat
already has that shape without the runtime. ``ljos-policyd`` (or
``phronesis_check_shell`` behind it) warns at the boundary. A policyd that is
missing or slow does not stop the work: the command runs and ljos logs a warning.
``ljos doctor`` names each habitat and whether it answers,
and a missing one is reported rather than guessed around. The claim graph
lives in ``$XDG_RUNTIME_DIR`` so a reboot clears leases instead of
resurrecting stale locks. A new plugin therefore looks like a habitat:
one store, one CLI, one Model Context Protocol (MCP) surface, doctor-visible. That is
cheaper than an OTP tree and keeps every store a file a person can read.

**herdr as the tooling integration: yes, and it is the ongoing work.**
``herdr`` is the session multiplexer the seat already speaks: personas
reason in a herdr pane when herdr is up and fall back to a tmux window
when it is not (``crates/ljos-cli/src/persona_session.rs``), the runner
works in the persona's home as its own seat, and the person's watch-and-talk
window is that same pane. The direction is more of this same
system: herdr panes as supervised seats, and policyd warning on writes
to the seat's own files on any runner. The :doc:`build harness <grok-build>` page is the reference
integration; each runner registers the events its table names.

Deep runner integration
=======================

``ljos onboard --harness NAME`` registers the MCP server, the hooks and the
skill for one runner. ``NAME`` is a name ``ljos onboard --example`` prints, among them
``antigravity``, ``opencode``, ``omp``, ``hermes``, ``copilot``, ``gemini``,
``windsurf``, ``factory``, ``kiro``, ``cline``, ``qwen``, ``crush``, ``zed``, ``vscode``,
``claude-desktop``, ``amazonq``, ``aider`` (conventions file, advisory), ``auggie``, ``amp``, ``goose``, ``openhands``,
``continue``, ``roo``, ``kilo``, ``example-host`` or ``shell``. The contract is the
same everywhere and the envelope differs:

+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Runner                   | Memory injection                                                                                     | Shell check                                                                                | Warning shows as                                        | Notes                                                                                                |
+==========================+======================================================================================================+============================================================================================+=========================================================+======================================================================================================+
| the terminal harness     | ``UserPromptSubmit`` context, skills, ``ljos://protocol``                                            | ``PreToolUse``, a rule warns                                                               | ``additionalContext``                                   | plugin ``ljos@leidarljos``; sitting and finish commands are in the repo                              |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| the command harness      | hooks plus MCP server list                                                                           | ``PreToolUse``, a rule warns                                                               | ``additionalContext``                                   | prompt and ``PreToolUse``; ``shell_command`` payloads carry the command and session ``cwd`` only, so |
|                          |                                                                                                      |                                                                                            |                                                         | ljos cannot read a ``workdir`` override                                                              |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| the build harness        | ``PostToolUse`` ``additionalContext`` after the first tool (``UserPromptSubmit`` stdout is           | ``PreToolUse``, 10 seconds, the Trusted Computing Base (TCB) check's budget                | ``additionalContext``                                   | frozen file ``crates/ljos-cli/assets/grok/ljos.json``; ``LJOS_MCP_GENERATION`` bump respawns the     |
|                          | discarded, so ``ljos hook`` keeps the text and emits it once)                                        |                                                                                            |                                                         | server; the full page is the build harness                                                           |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Antigravity (``agy``)    | hooks plus skill                                                                                     | ``PreToolUse``, a rule warns                                                               | the ``reason`` of ``decision: allow``                   | pipeline stages arrive separated by a ``\|`` word so a download and the shell it feeds are judged    |
|                          |                                                                                                      |                                                                                            |                                                         | together                                                                                             |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| opencode                 | ``assets/opencode/ljos.ts``                                                                          | bash calls, a rule warns                                                                   | context for the next model call                         | same contract, TypeScript hook                                                                       |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| omp                      | ``assets/omp/ljos.ts``                                                                               | bash calls, a rule warns                                                                   | context for the next model call                         | same contract, TypeScript hook                                                                       |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| hermes                   | MCP server list plus skill                                                                           | advisory: onboard registers no hook                                                        | none: no hook                                           | nothing leaves the machine unless a remote judge is turned on                                        |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| the editor harness       | prompt note on the first tool result; skill; ``~/.cursor/agents``                                    | ``beforeShellExecution`` and ``preToolUse``; a missing policyd allows                      | ``user_message`` and ``agent_message`` beside ``allow`` | the IDE and the ``agent`` CLI share ``~/.cursor/mcp.json`` and ``~/.cursor/hooks.json``              |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| GitHub Copilot CLI       | skill; no prompt note, since Copilot drops a command hook's prompt output                            | ``PreToolUse`` in ``~/.copilot/hooks/ljos.json``                                           | stderr                                                  | the same file in a repository's ``.github/hooks`` reaches Copilot's cloud runner                     |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Gemini CLI               | ``BeforeAgent`` ``additionalContext``                                                                | ``BeforeTool`` on ``run_shell_command``                                                    | ``additionalContext``                                   | hooks and MCP server share ``~/.gemini/settings.json``; timeouts there are milliseconds              |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Windsurf (Devin Desktop) | skill                                                                                                | ``pre_run_command``                                                                        | stderr                                                  | ``~/.codeium/windsurf/hooks.json``                                                                   |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Factory droid            | ``UserPromptSubmit`` context                                                                         | ``PreToolUse`` on ``Execute``                                                              | ``additionalContext``                                   | ``~/.factory/hooks.json``, keyed by event at the top level                                           |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Kiro                     | skill                                                                                                | ``PreToolUse`` on the ``shell`` tools                                                      | stdout text                                             | ``~/.kiro/hooks/ljos.json``, Kiro CLI V3 and IDE 1.0                                                 |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Cline                    | ``UserPromptSubmit`` ``contextModification``                                                         | ``PreToolUse``, ``cancel: false``                                                          | ``contextModification``                                 | two scripts in ``~/Documents/Cline/Hooks``, macOS and Linux                                          |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Qwen Code                | ``UserPromptSubmit`` context                                                                         | ``PreToolUse`` on ``run_shell_command``                                                    | ``additionalContext``                                   | ``~/.qwen/settings.json``                                                                            |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Crush                    | skill                                                                                                | ``PreToolUse`` on ``bash``                                                                 | ``context``                                             | ``~/.config/crush/crush.json``; Crush fires only ``PreToolUse`` so far                               |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Auggie                   | skill                                                                                                | ``PreToolUse`` on ``launch-process``                                                       | ``additionalContext``                                   | ``~/.augment/settings.json`` runs ``~/.augment/ljos-guard-auggie.sh``                                |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Amp                      | skill                                                                                                | ``Bash`` delegated first in ``amp.permissions``, exit 0                                    | stderr                                                  | ``~/.config/amp/settings.json`` runs ``~/.config/amp/ljos-guard-amp.sh``                             |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| goose                    | skill in the plugin                                                                                  | ``PreToolUse`` on ``shell``                                                                | stderr                                                  | user plugin ``~/.agents/plugins/ljos``; server in ``~/.config/goose/config.yaml``                    |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| OpenHands                | ``UserPromptSubmit`` ``additionalContext``                                                           | ``PreToolUse`` on ``terminal``                                                             | ``additionalContext``                                   | ``~/.openhands/hooks.json`` and ``~/.openhands/mcp.json``                                            |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Continue                 | rules file; ``SessionStart`` hook                                                                    | advisory: hooks in ``~/.continue/settings.json`` are written, cn 1.5.47 does not fire them | none: no hook fires                                     | ``~/.continue/mcpServers/ljos.json``                                                                 |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Roo Code, Kilo Code      | skill                                                                                                | advisory: no tool hook                                                                     | none: no tool hook                                      | the extension's ``mcp_settings.json``                                                                |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+
| Example Host             | the skill, then ``ljos hook --prompt``                                                               | ``ljos policy`` prints the line                                                            | a ``warning:`` line                                     | no MCP server and no hook file; source the env file ``onboard`` writes                               |
+--------------------------+------------------------------------------------------------------------------------------------------+--------------------------------------------------------------------------------------------+---------------------------------------------------------+------------------------------------------------------------------------------------------------------+

The command harness runs a hook only after you trust it. After ``ljos onboard --harness codex``,
open ``/hooks`` and trust each ljos hook before it runs.
Trust it again after an upgrade rewrites one. Until then the hook does not run,
and policyd sees none of the command harness's commands. ``ljos doctor`` reports an untrusted hook when
it can see one.

The runners from Copilot down call ``ljos hook --runner NAME``. That
command reads the runner's payload as the terminal harness's, runs the same hook,
and writes the answer back in the runner's field names, with exit 0. By default the answer
never denies, asks or blocks, apart from a cloud seat's push rule; a rule that fires is one
warning line in the column above. That push rule's refusal, and policyd's block on a tool call
under the person's ``block-on-warning``, is that runner's own deny, with exit 2 where the runner
reads a deny from the exit code.
On a tool call, the relay lets the command run and logs a warning in three cases.
The inner hook crashes, exits non-zero without an answer, or runs past ``GUARD_TIMEOUT``.
That is 5 seconds unless ``LJOS_GUARD_TIMEOUT`` names another number of seconds.
A policyd that is missing, hangs, crashes, or prints no answer does not
stop the command either. ``POLICYD_REQUIRED=1`` makes a missing binary a failing ``ljos doctor`` row; the line still runs.
On any other event, such as a prompt, the relay waits 9 seconds, past
the inner hook's own 8 second deadline, and a fault lets the event
through with a note.

When a hook fails
-----------------

A check works only when its hook runs. What happens when the hook breaks is the runner's call,
and the runners disagree. The table says what
happens to the command when the seat's hook crashes, times out, or
prints something the runner cannot read. "Runs" means the command goes
ahead unchecked.

======================== ================================================ ============== ============================================== ====================================================================================================================================================================================================
Runner                   Hook exits non-zero (not 2) or crashes           Hook times out Answer is not JSON                             Source
======================== ================================================ ============== ============================================== ====================================================================================================================================================================================================
the terminal harness     runs; only exit 2 blocks                         runs           runs                                           `hooks reference <https://docs.claude.com/en/docs/claude-code/hooks>`__
the command harness      runs; only exit 2 with a reason on stderr blocks runs           runs                                           `hooks <https://developers.openai.com/codex/hooks>`__, `pre\ tooluse.rs <https://github.com/openai/codex/blob/de8fab6d7adfcef8b4ce6f02f3b5c8be4092015a/codex-rs/hooks/src/events/pre_tool_use.rs>`__
the editor harness       runs: the seat does not set ``failClosed``       runs           ``beforeShellExecution`` blocks it             the editor harness hooks docs
GitHub Copilot CLI       blocked                                          runs           falls through to Copilot's own permission flow `hooks reference <https://docs.github.com/en/copilot/reference/hooks-reference>`__
Gemini CLI               runs, with a warning                             not documented runs                                           `hooks reference <https://geminicli.com/docs/hooks/reference/>`__
Windsurf (Devin Desktop) runs; only exit 2 blocks                         not documented not read; the exit code decides                `Cascade hooks <https://docs.devin.ai/desktop/cascade/hooks>`__
Factory droid            runs; only exit 2 blocks                         not documented not read; the exit code decides                `hooks <https://docs.factory.ai/harness/hooks>`__
Kiro                     blocked                                          not documented not read; the exit code decides                `hook actions <https://kiro.dev/docs/hooks/actions/>`__
Cline                    not documented                                   not documented not documented                                 `cline/cline <https://github.com/cline/cline>`__
Qwen Code                runs; only exit 2 blocks                         not documented not read; the exit code decides                `hooks <https://qwenlm.github.io/qwen-code-docs/en/users/features/hooks/>`__
Crush                    runs; only exit 2 blocks                         runs           not read; the exit code decides                `hooks <https://github.com/charmbracelet/crush/blob/main/docs/hooks/README.md>`__
======================== ================================================ ============== ============================================== ====================================================================================================================================================================================================

The seat does not turn a fault into a refusal. A ``ljos-policyd`` that is missing, hangs, crashes,
or prints no answer lets the command run, and ljos logs a warning. So do a relay that cannot read its
input and a wait past ``LJOS_GUARD_TIMEOUT``. CI and a fresh install are never locked out.
The hook files the seat writes give each runner a
timeout of 10 to 20 seconds. A check that takes longer meets the
"times out" column. None of this helps when the ``ljos`` binary itself
is gone. On Copilot and Kiro that blocks every shell command, and on
every other runner the commands run unchecked.

When the seat's own hook fails on a permission event, it writes an allow in the runner's JSON, so a
runner that blocks on a missing answer still runs the command.

Two rules hold on every runner.
First, a command that writes the seat's own files, its binaries, its hook entries and its policy files, runs with the ``writes-seat-file`` warning, and the bypass log keeps the label.
That warning is the only thing that tells the person the law the runner works under was rewritten.
ljos adds one more warning, for a command or a file tool that writes the tracker's files by hand.
Second, ``ljos policy -- COMMAND`` prints the answer for one command without running it,
so every integration is testable without the runner. ``scripts/smoke.sh`` does exactly that.

policyd can also answer a command with a fixed one. The new line runs only
when the guard has no warning on it. The harnesses that accept an updated command, including the editor harness's ``preToolUse``, take it
through the hook's updated input. Every other runner in the table runs the old line, with a
warning that names the new command.

Dogfooding
==========

This repository eats its own cooking. Sittings are claimed through the
claim graph, lessons are remembered with ``ljos remember`` and reviewed
with ``ljos due``, panel decisions are settled with ``ljos consensus`` and
calibrated with ``ljos calibrate``, and releases cite their deeds. When an
integration breaks, the breakage is filed as an issue in the tracker
first, because a conversation that has no issue is told to file one
and sit. The measure of this page is whether the next runner takes an
afternoon: one envelope, the events that runner names, one contract.
