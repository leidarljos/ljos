How the seat meets the rest of the world: the memory systems it learns
from, the concurrency designs it voted on, the runners it sits in, and
the dogfooding that keeps it honest. The :doc:`explanation` says what the seat is; this page says what it plugs into and why.

Where the seat stands among FOSS memory systems
===============================================

Agent memory systems in the literature and on the market do one of a few
jobs. The seat integrates with them rather than re-running them:

- Mem0 (doi:10.48550/arXiv.2504.19413) reads a conversation with a model,
  extracts facts, and decides per fact whether to add, update, delete or
  leave the store. The seat's proposals path is the gated equivalent: a
  proposal becomes a claim only on an explicit accept, and nothing is
  mined silently from a transcript.
- Zep (doi:10.48550/arXiv.2501.13956) builds a temporal knowledge graph
  with edges carrying the time an edge became true and stopped being true,
  plus fused lexical, dense and graph retrieval. The pack carries the same
  two times on every claim (written-at and the live window a superseding
  claim closes) and fuses a prefix scan, BM25+ and a dense ballot; a Zep
  graph can be read as trust rows and island links, not as a second store.
- Letta, formerly MemGPT (doi:10.48550/arXiv.2310.08560), with sleep-time
  compute (doi:10.48550/arXiv.2504.13171), keeps a small core memory the
  model edits with tools, plus recall and archival stores reorganised
  between turns. The seat's review clock (``ljos due``, ``graded``) is the
  reorganisation, but it runs on FSRS retrievability rather than on a model
  pass, so it works with no model at all.
- HippoRAG (doi:10.48550/arXiv.2405.14831, doi:10.48550/arXiv.2502.14802)
  retrieves a knowledge graph by personalised PageRank from the entities a
  question names. The island (``ljos island``) is the same idea over Hebbian
  link weights, read beside the fused hits for orientation rather than in
  their place, because measured as a ranking it lost; see the explanation.
- MemoryBank (doi:10.48550/arXiv.2305.10250) forgets on an Ebbinghaus curve
  refreshed by recall; Generative Agents (doi:10.48550/arXiv.2304.03442)
  rank by recency, importance and relevance plus reflection. Forgetting here
  is by review, not by age: FSRS retrievability scales search, and a
  never-recalled lesson lapses like a missed review.
- Seldon (`seldon-code/seldon <https://github.com/seldon-code/seldon>`__)
  is the ODE engine for opinion dynamics. ``ljos-consensus settle --seldon``
  writes its inputs and reads its output, so the discrete step is checked
  against a second implementation without linking its GPL code.

The one design choice the others do not make is the privacy of write:
what is remembered is what was said with ``Remember`` or ``Prefer``,
stored as written. Integration never crosses that line: foreign facts
arrive as proposals, trust rows, or cited deeds, each attributable and
retractable.

The concurrency vote: Go, Erlang-style, or herdr
================================================

Three designs were on the table for the seat's concurrent life: many
agents writing at once. The vote went as follows, and the code follows it.

**A Go rewrite for goroutines: no.** The bottleneck is not compute, it is
one writer. The pack is one daemon owning one LMDB file (``packsetd``);
the claim graph is one in-process graph with the host as sole mutator
(``claimdag``); deed appends take a file lock so two processes minting at
once write whole lines. The packset README records zero errors at 32
clients, and a herd of 32 writing 6400 distinct claims into one workspace
keeps 6382 live and 18 closed. Goroutines parallelise what is already fast and leave the single
writer single; a rewrite would trade the Rust type-level guarantees (closed
enums for kind, status and role; CAS on ``generation``) for a runtime that
does not change the contention shape. Concurrency here is a hammer test,
not a port: ``packsetd`` serves many readers, one writer, and the hammer
rerun is the regression gate.

**An Erlang-style plugin tree: no as an OTP port, yes as the pattern.**
What Erlang would buy is supervision: a crashed worker restarts, a bad
message is refused at the boundary, fail-closed by default. The seat
already has that shape without the runtime: ``ljos-policyd`` (or
``phronesis_check_shell`` behind it) refuses at the boundary and protocol
failures deny; ``ljos doctor`` names each habitat and whether it answers,
and a missing one is reported rather than guessed around; the claim graph
lives in ``$XDG_RUNTIME_DIR`` so a reboot clears leases instead of
resurrecting stale locks. A new plugin therefore looks like a habitat: one
store, one CLI, one MCP surface, fail-closed, doctor-visible. That is
cheaper than an OTP tree and keeps every store a file a person can read.

**herdr as the tooling integration: yes, and it is the ongoing work.**
``herdr`` is the session multiplexer the seat already speaks: personas
reason in a herdr pane when herdr is up and fall back to a tmux window when
it is not (``crates/ljos-cli/src/persona_session.rs``), the runner works in
the persona's home as its own seat, and the person's watch-and-talk window
is that same pane. The direction is more of this, not a new system: herdr
panes as supervised seats, the seat guard refusing writes to the seat's own
files, approvals flowing through ``ljos approve`` on any runner. The
:doc:`Grok Build <grok-build>` page is the reference integration; every
other harness registers the events its table names.

Deep harness integration
========================

``ljos onboard --harness NAME`` registers the MCP server, the hooks and the
skill for one runner. ``NAME`` takes ``claude``, ``codex``, ``grok``, ``cursor``,
``antigravity``, ``opencode``, ``omp``, ``hermes``, ``windsurf``, ``zed``,
``vscode``, ``claude-desktop``, ``gemini``, ``amazonq``, ``kiro``, ``grokbot``
or ``shell``. The contract is the same everywhere and the envelope differs:

- Claude Code: ``UserPromptSubmit`` context, skills, ``ljos://protocol``;
  ``PreToolUse`` deny blocks; ask is the native permission prompt; the
  plugin ``ljos@leidarljos`` ships the sitting and finish commands.
- Codex: hooks plus MCP server list; hook deny blocks; ask is the runner
  prompt or ``ljos approve``; the prompt and ``PreToolUse``.
- Grok Build: ``PostToolUse`` ``additionalContext`` after the first tool
  (``UserPromptSubmit`` stdout is discarded, so ``ljos hook`` holds the
  text and emits it once); ``PreToolUse`` gets 10 seconds, the TCB budget;
  ``ask`` is rewritten to deny where stdin carries ``turn_id``; the frozen
  file is ``crates/ljos-cli/assets/grok/ljos.json`` and the
  ``LJOS_MCP_GENERATION`` bump respawns the server. The full page is
  :doc:`Grok Build <grok-build>`.
- Antigravity (``agy``): hooks plus skill; hook deny blocks; approvals via
  ``ljos approve``, ``ljos_request_approval``, or reply ``approve ID``;
  pipeline stages arrive separated by a ``|`` word so a download and the
  shell it feeds are judged together.
- opencode: ``assets/opencode/ljos.ts``; hook deny blocks; ask is the
  runner prompt or ``ljos approve``.
- omp: ``assets/omp/ljos.ts``; hook deny blocks; ask is the runner prompt
  or ``ljos approve``.
- hermes: MCP server list plus hooks; hook deny blocks; ``ljos approve``;
  nothing leaves the machine unless a remote judge is turned on.
- Cursor: the IDE and the ``agent`` CLI share ``~/.cursor/mcp.json`` and
  ``~/.cursor/hooks.json``; a prompt note arrives with the first tool
  result; ``beforeShellExecution`` and ``preToolUse`` fail closed; an ask
  on a shell command is Cursor's permission prompt.
- Grok Bot: no MCP server and no hook file. ``ljos onboard --harness grokbot``
  writes the skill and an env file to source, so ``LJOS_SEAT`` is the seat.
  ``ljos hook --prompt`` is the prompt hook. ``ljos policy --fail-on-deny``
  exits 1 on a deny. Without the flag the exit stays 0.

Two rules hold on every runner. First, the seat guard refuses a command
that writes the seat's own files (its binaries, its hook entries,
``~/.config/ljos``, the approvals store): an agent cannot rewrite the law
it runs under. Second, ``ljos policy -- COMMAND`` prints the combined
answer for one command without running it, so every integration is testable
without the runner: ``scripts/smoke.sh`` does exactly that.

Dogfooding
==========

This repository eats its own cooking. Sittings are claimed through the
claim graph, lessons are remembered with ``ljos remember`` and reviewed
with ``ljos due``, panel decisions are settled with ``ljos consensus`` and
calibrated with ``ljos calibrate``, and releases cite their deeds. When an
integration breaks, the breakage is filed as an issue in the tracker first,
because a conversation that holds no issue is told to file one and sit. The
measure of this page is whether the next harness takes an afternoon: one
envelope, the events that runner names, the same contract.
