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
| Mem0 (doi:10.48550/arXiv.2504.19413)                                                                            | a model extracts facts from a conversation and decides per fact whether to add, update, delete or leave the store                           | the seat's proposals path does the same behind a check: a proposal becomes a claim only on an explicit accept, and the seat never mines a   |
|                                                                                                                 |                                                                                                                                             | transcript on its own                                                                                                                       |
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
proposals, trust rows, or cited deeds, each attributable and retractable.

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
What Erlang would buy is supervision: a crashed worker restarts, a bad
message is refused at the boundary, fail-closed by default. The seat
already has that shape without the runtime: ``ljos-policyd`` (or
``phronesis_check_shell`` behind it) refuses at the boundary and protocol
failures deny; ``ljos doctor`` names each habitat and whether it answers,
and a missing one is reported rather than guessed around; the claim graph
lives in ``$XDG_RUNTIME_DIR`` so a reboot clears leases instead of
resurrecting stale locks. A new plugin therefore looks like a habitat:
one store, one CLI, one Model Context Protocol (MCP) surface, fail-closed, doctor-visible. That is
cheaper than an OTP tree and keeps every store a file a person can read.

**herdr as the tooling integration: yes, and it is the ongoing work.**
``herdr`` is the session multiplexer the seat already speaks: personas
reason in a herdr pane when herdr is up and fall back to a tmux window
when it is not (``crates/ljos-cli/src/persona_session.rs``), the runner
works in the persona's home as its own seat, and the person's watch-and-talk
window is that same pane. The direction is more of this same
system: herdr panes as supervised seats, the seat guard refusing writes
to the seat's own files, approvals flowing through ``ljos approve`` on any
runner. The :doc:`Grok Build <grok-build>` page is the reference
integration; each runner registers the events its table names.

Deep runner integration
=======================

``ljos onboard --harness NAME`` registers the MCP server, the hooks and the
skill for one runner. ``NAME`` takes ``claude``, ``codex``, ``grok``, ``cursor``,
``antigravity``, ``opencode``, ``omp``, ``hermes``, ``copilot``, ``gemini``,
``windsurf``, ``factory``, ``kiro``, ``cline``, ``qwen``, ``crush``, ``zed``, ``vscode``,
``claude-desktop``, ``amazonq``, ``grokbot`` or ``shell``. The contract is the
same everywhere and the envelope differs:

+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Runner                   | Memory injection                                                                                           | Shell check                                                                 | Ask                                                                  | Notes                                                                                                      |
+==========================+============================================================================================================+=============================================================================+======================================================================+============================================================================================================+
| Claude Code              | ``UserPromptSubmit`` context, skills, ``ljos://protocol``                                                  | ``PreToolUse`` deny blocks                                                  | native permission prompt                                             | plugin ``ljos@leidarljos``; sitting and finish commands are in the repo                                    |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Codex                    | hooks plus MCP server list                                                                                 | hook deny blocks                                                            | runner prompt or ``ljos approve``                                    | prompt and ``PreToolUse``                                                                                  |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Grok Build               | ``PostToolUse`` ``additionalContext`` after the first tool (``UserPromptSubmit`` stdout is discarded, so   | ``PreToolUse``, 10 seconds, the Trusted Computing Base (TCB) check's budget | ``ask`` rewritten to deny where stdin carries ``turn_id``            | frozen file ``crates/ljos-cli/assets/grok/ljos.json``; ``LJOS_MCP_GENERATION`` bump respawns the server;   |
|                          | ``ljos hook`` keeps the text and emits it once)                                                            |                                                                             |                                                                      | the full page is Grok Build                                                                                |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Antigravity (``agy``)    | hooks plus skill                                                                                           | hook deny blocks                                                            | ``ljos approve``, ``ljos_request_approval``, or reply ``approve ID`` | pipeline stages arrive separated by a ``\|`` word so a download and the shell it feeds are judged together |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| opencode                 | ``assets/opencode/ljos.ts``                                                                                | hook deny blocks                                                            | runner prompt or ``ljos approve``                                    | same contract, TypeScript hook                                                                             |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| omp                      | ``assets/omp/ljos.ts``                                                                                     | hook deny blocks                                                            | runner prompt or ``ljos approve``                                    | same contract, TypeScript hook                                                                             |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| hermes                   | MCP server list plus hooks                                                                                 | hook deny blocks                                                            | ``ljos approve``                                                     | nothing leaves the machine unless a remote judge is turned on                                              |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Cursor                   | prompt note on the first tool result; skill; ``~/.cursor/agents``                                          | ``beforeShellExecution`` and ``preToolUse``, fail closed                    | Cursor's permission prompt on a shell command                        | the IDE and the ``agent`` CLI share ``~/.cursor/mcp.json`` and ``~/.cursor/hooks.json``                    |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| GitHub Copilot CLI       | skill; no prompt note, since Copilot drops a command hook's prompt output                                  | ``PreToolUse`` in ``~/.copilot/hooks/ljos.json``, ``permissionDecision``    | Copilot's permission prompt                                          | the same file in a repository's ``.github/hooks`` reaches Copilot's cloud agent                            |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Gemini CLI               | ``BeforeAgent`` ``additionalContext``                                                                      | ``BeforeTool`` on ``run_shell_command``, ``decision: deny``                 | none: an ask is a deny that says so                                  | hooks and MCP server share ``~/.gemini/settings.json``; timeouts there are milliseconds                    |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Windsurf (Devin Desktop) | skill                                                                                                      | ``pre_run_command``, exit 2 with the reason on stderr                       | none                                                                 | ``~/.codeium/windsurf/hooks.json``                                                                         |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Factory droid            | ``UserPromptSubmit`` context                                                                               | ``PreToolUse`` on ``Execute``, ``permissionDecision``                       | droid's permission prompt                                            | ``~/.factory/hooks.json``, keyed by event at the top level                                                 |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Kiro                     | skill                                                                                                      | ``PreToolUse`` on the ``shell`` tools, exit 2 with the reason on stderr     | none                                                                 | ``~/.kiro/hooks/ljos.json``, Kiro CLI V3 and IDE 1.0                                                       |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Cline                    | ``UserPromptSubmit`` ``contextModification``                                                               | ``PreToolUse``, ``cancel: true``                                            | none                                                                 | two scripts in ``~/Documents/Cline/Hooks``, macOS and Linux                                                |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Qwen Code                | ``UserPromptSubmit`` context                                                                               | ``PreToolUse`` on ``run_shell_command``, ``permissionDecision``             | Qwen's confirmation                                                  | ``~/.qwen/settings.json``                                                                                  |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Crush                    | skill                                                                                                      | ``PreToolUse`` on ``bash``, ``decision: deny``                              | none                                                                 | ``~/.config/crush/crush.json``; Crush fires only ``PreToolUse`` so far                                     |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+
| Grok Bot                 | the skill, then ``ljos hook --prompt``                                                                     | ``ljos policy --fail-on-deny`` exits 1                                      | the person runs the command                                          | no MCP server and no hook file; source the env file ``onboard`` writes                                     |
+--------------------------+------------------------------------------------------------------------------------------------------------+-----------------------------------------------------------------------------+----------------------------------------------------------------------+------------------------------------------------------------------------------------------------------------+

The runners from Copilot down call ``ljos hook --runner NAME``. That
command reads the runner's payload as Claude Code's, runs the same hook,
and writes the answer back in the runner's field names and exit code.
The relay denies when the inner hook crashes, exits non-zero without
an answer, or runs past 12 seconds. The reason names the failure and
says to run ``ljos doctor``.

When a hook fails
-----------------

A guard is only as strong as what the runner does when the hook breaks.
Each runner decides that itself, and they disagree. The table says what
happens to the command when the seat's hook crashes, times out, or
prints something the runner cannot read. "Runs" means the command goes
ahead unchecked.

======================== ==================================================================================== ============== ============================================== ======================================================================================================================
Runner                   Hook exits non-zero (not 2) or crashes                                               Hook times out Answer is not JSON                             Source
======================== ==================================================================================== ============== ============================================== ======================================================================================================================
Claude Code              runs; only exit 2 blocks                                                             runs           runs                                           `hooks reference <https://docs.claude.com/en/docs/claude-code/hooks>`__
Codex                    runs; only exit 2 with a reason on stderr blocks                                     runs           runs                                           `hooks <https://developers.openai.com/codex/hooks>`__, `pre_tool_use.rs <https://github.com/openai/codex/blob/de8fab6d7adfcef8b4ce6f02f3b5c8be4092015a/codex-rs/hooks/src/events/pre_tool_use.rs>`__
Cursor                   runs, unless the hook sets ``failClosed``; the seat sets it on the permission events not checked    blocked on ``beforeShellExecution``            Cursor hooks docs
GitHub Copilot CLI       blocked                                                                              runs           falls through to Copilot's own permission flow `hooks reference <https://docs.github.com/en/copilot/reference/hooks-reference>`__
Gemini CLI               runs, with a warning                                                                 not documented runs                                           `hooks reference <https://geminicli.com/docs/hooks/reference/>`__
Windsurf (Devin Desktop) runs; only exit 2 blocks                                                             not documented not read; the exit code decides                `Cascade hooks <https://docs.devin.ai/desktop/cascade/hooks>`__
Factory droid            runs; only exit 2 blocks                                                             not documented runs                                           `hooks <https://docs.factory.ai/harness/hooks>`__
Kiro                     blocked                                                                              not documented not read; the exit code decides                `hook actions <https://kiro.dev/docs/hooks/actions/>`__
Cline                    not documented                                                                       not documented not documented                                 `cline/cline <https://github.com/cline/cline>`__
Qwen Code                runs; only exit 2 blocks                                                             not documented runs                                           `hooks <https://qwenlm.github.io/qwen-code-docs/en/users/features/hooks/>`__
Crush                    runs; only exit 2 blocks                                                             runs           not documented                                 `hooks <https://github.com/charmbracelet/crush/blob/main/docs/hooks/README.md>`__
======================== ==================================================================================== ============== ============================================== ======================================================================================================================

Three things on the seat's side close part of the gap. ``POLICYD_REQUIRED=1``
makes a missing ``ljos-policyd`` a deny instead of an allow. The hook
files the seat writes give the runner a timeout of 10 to 20 seconds; a
check that takes longer meets the "times out" column. On the runners
from Copilot down, ``ljos hook --runner`` denies when the hook it runs
crashes, exits non-zero without an answer, or runs past 12 seconds, so
those failures reach the runner as a deny. None of these helps when
the ``ljos`` binary itself is gone: on Copilot and Kiro that blocks every
shell command, and on every other runner the commands run unchecked.

Two rules hold on every runner. First, the seat guard refuses a command
that writes the seat's own files (its binaries, its hook entries,
``~/.config/ljos``, the approvals store): an agent cannot rewrite the law
it runs under. Second, ``ljos policy -- COMMAND`` prints the combined
answer for one command without running it, so every integration is
testable without the runner: ``scripts/smoke.sh`` does exactly that.

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
