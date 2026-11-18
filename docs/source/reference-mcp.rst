:description: The tools the Model Context Protocol (MCP) server ljos-mcp serves, the core set and how to list every tool.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=========
MCP tools
=========


The seat serves these over the Model Context Protocol (MCP).

Writers: ``ljos_start``, ``ljos_file``, ``ljos_options``, ``ljos_note``, ``ljos_sitting``, ``ljos_finish``, ``ljos_calibrate``, ``ljos_persona``, ``ljos_playbook``, ``ljos_remember``, ``ljos_prefer``, ``ljos_forget``, ``ljos_trust``,
``ljos_learn``, ``ljos_graded``, ``ljos_island``, ``ljos_deed``, ``ljos_vote``, ``ljos_predict``, ``ljos_rule``, ``ljos_claim``,
``ljos_release``, ``ljos_complete``, ``ljos_consolidate``, ``ljos_findings``, ``ljos_bump_plan``, ``ljos_handover``, ``ljos_receive``, ``ljos_send``, ``ljos_inbox``, ``ljos_read``, ``ljos_reply``, ``ljos_group``. Readers: ``ljos_search`` (with ``query`` and optional ``as_of``), ``ljos_guard_check``, ``ljos_conflicts``, ``ljos_show``,
``ljos_timeline``, ``ljos_digest``, ``ljos_due``, ``ljos_habit`` (a writer with a value), ``ljos_evidence``, ``ljos_current``, ``ljos_recall``, ``ljos_ready``,
``ljos_consensus``, ``ljos_brief``, ``ljos_personas``, ``ljos_playbooks``, ``ljos_cards``, ``ljos_policy``, ``ljos_doctor``, ``ljos_mail_search``, ``ljos_mail_show``. Resources:
``ljos://protocol``, ``ljos://cards/USER.md``, ``ljos://cards/MEMORY.md``. Prompts:
``start_a_sitting``, ``run_a_panel``, ``check_a_handover``. Most tool descriptions open with
when to call it. The resources are Markdown: the listing and a read both say ``text/markdown``. Every description in the tool, prompt and resource listings is one line.

A writer needs a seat, and a reader runs with none. Seven writers read until a call asks them to write: ``ljos_deed`` without ``add``, ``ljos_vote`` with no ballot, ``ljos_bump_plan`` with ``dry_run``, ``ljos_habit`` with no ``value``, ``ljos_findings`` with neither ``remember`` nor ``issue``, ``ljos_group`` that only lists, and ``ljos_island`` without ``fire``. Each tool's ``seat`` argument says which kind it is: optional on a reader, needed on a shared server for a writer, and needed when the call writes for these seven.

``ljos_mail_search`` and ``ljos_mail_show`` read the owner's mailbox through notmuch. They are not in the core set, so ask for them with ``LJOS_MCP_TOOLS=core,mail_search,mail_show``. Both refuse until a ``[mail]`` table in the user's own ``ljos.toml`` names ``notmuch_config``, and a cloud seat refuses them. The `mail design <https://github.com/leidarljos/ljos/blob/develop/docs/design/mail.md>`__ has the table and the content checks.

By default the server lists the core set, in this order: ``ljos_rule``,
``ljos_prefer``, ``ljos_remember``, ``ljos_search``,
``ljos_start``, ``ljos_guard_check``, ``ljos_file``, ``ljos_options``, ``ljos_note``, ``ljos_recall``, ``ljos_show``, ``ljos_ready``,
``ljos_sitting``, ``ljos_finish``, ``ljos_claim``, ``ljos_release``, ``ljos_send``,
``ljos_inbox``, ``ljos_reply``, ``ljos_vote``, ``ljos_consensus`` and ``ljos_doctor``.
``LJOS_MCP_TOOLS=all`` lists every tool above: the core set first, then the rest
by name, then the persona and playbook tools.
A description in the core set names only tools the core set lists, so a call it suggests is one the server answers. ``ljos_finish`` frees a claim and names the outcome that scores a vote.
``LJOS_MCP_TOOLS`` also takes names split on commas or spaces, with or
without the ``ljos_`` prefix, where ``core`` stands for the core set. A call
to a tool the server does not list is refused. The line asks the person to set ``LJOS_MCP_TOOLS`` and restart the server, and names the shell command to run meanwhile when the tool has one. A tool that fails answers with ``isError`` set and the reason
as text, so the model reads it as a tool result.
``tools/list`` does not wait on a seat, a tracker, ``PACKSET_HOME``, or a running
``packsetd``, so a fresh machine still receives the whole set it asked for.
A search or another pack tool with no writer, or a writer that answers 401,
is that tool error and names ``cargo install --locked packset`` or
``packset ensure``. It is not a protocol error, and it does not drop the list.
The server stays up when ``packsetd`` restarts, when a line is not JSON, and while the client is quiet.
A dead socket is opened again on the same call.
A failure is one line and names the command.
A missing tracker names ``ljos onboard``.
A busy pack names ``ljos search`` or ``ljos remember``.
An unknown tool names ``tools/list`` and ``ljos help``.
Arguments that do not parse name the field that failed and the ``ljos help`` page for the tool.
A failed call ends with a ``next:`` line that names the tool to call again once the line above is fixed, and ``ljos_guard_check`` adds that the command is not to run.
An unknown method, or a line that is not JSON, names ``ljos mcp --help``.
Setting ``LJOS_SEAT`` applies to reads only; a write on a shared ljos mcp server with no ``seat`` argument and no stamped session is refused and names ``ljos seat``.

A result whose text starts with ``label`` is a refusal that is not an ``isError`` result, such as ``seat-unlisted`` or ``claim-conflict``. The call did not do its work, and the ``next:`` line says to follow the step the label names and call the tool again.
Every tool that takes an issue says the id is the tracker id ``ljos_ready`` prints, such as ``proj-1a2b``. ``ljos_vote`` takes any option from the issue's ``Options:`` line in ``choice``, such as ``keep``, ``drop``, ``accept``, or ``reject``, and ``ljos_options`` writes that line.

``ljos_claim`` takes ``takeover`` set to true to take a ticket over from another seat whose claim looks stale, the step ``ljos claim ID --takeover`` takes. A holder that is running, or whose claim moved inside the lease, is refused with the ``claim-conflict`` label and nothing changes. The old holder is named in a note on the ticket. ``ljos_start`` and ``ljos_sitting`` already claim, and the server claims on the first write, so call ``ljos_claim`` only to name the issue yourself. A named worker occupies one slot, and a ``busy`` refusal names the held node. ``ljos_finish`` or ``ljos_release`` frees it. ``ljos_deed`` is the same: a git commit is cited as a deed for you, so call it for the other deeds.

``ljos_start`` is the short opening: it recalls and claims. ``ljos_sitting`` is the full opening in the protocol's order, and stops at the first store that does not answer. Pass ``issue``, or ``plan`` with ``items`` to file the checklist in one tracker commit. ``playbook`` is one of ``sit``, ``arena``, ``land``, ``company-panel``, ``overnight`` or ``feature``, and stays bound until ``ljos_finish`` or ``ljos_release``. Leave ``assignee`` out unless the call runs as a named worker.

``ljos_rule`` takes a ``verdict`` of ``deny`` or ``ask``, in any case. Both are labels on a warning, and neither stops the command. Any other verdict is refused with ``isError`` set and nothing is written. A person writes an ``allow`` rule at their own terminal.

``ljos_show`` prints the full ticket page ``ljos show`` prints: the header with the wrapped title, the claim if any, parent and children, blockers, the full body, and the latest notes.

``ljos_search`` ranks the seat's memory for a query. Its tool description is short on purpose, so the rules it leaves out are here. Each row carries a score from 0 to 100 that ranks the match and does not say the claim is true. This seat's rows come first. Another seat's row is kept when it names the subject, or when the encoder's cosine reaches 0.66, and a single generic word is not a subject. A short list is not padded with a weaker cross-seat row, and ``min_score`` (34 by default, ``0`` returns every row) drops a row under that score. A pack that is still starting, or stays busy, gives word matches and the reading says so, for example ``Used word search because memory is still starting``. A search with ``as_of`` has no word fallback and answers ``Memory is starting, so try again in a few seconds``. A failure means the pack writer is down, and ``ljos_doctor`` says why.

``ljos_ready`` prints the same first page as ``ljos ready``. The default is 20 tickets. ``limit`` changes that count and ``all`` prints every ticket. The last line of a short page names ``--limit`` and ``--all``. This seat's claims come first, then unclaimed work by priority and recent activity, then other seats' claims, with long-stale claims last. The id breaks the final tie.

``ljos_policy`` prints an argument list as it would run, with the ruling a rule gives it, and runs nothing. It is not a check. ``ljos_guard_check`` takes a shell line and is the one to call before running a command. ``ljos_conflicts`` lists pairs of memories that may contradict each other and writes nothing. ``ljos_graded`` takes ``recalled``, which is true by default and pushes the next review out; false brings it back sooner.

``ljos_doctor`` returns one row for each habitat with ``name``, ``state``, ``ok`` and ``optional``. A row with ``optional`` true is an extra the seat runs without, such as ``packset-embed``, ``ljos-hud``, the cloud guard, the runners or the git hooks, so ``ok`` false there is not a failure. It is the row ``ljos doctor`` prints as ``info``. A row with ``optional`` false and ``ok`` false needs a step. The install command a row names does not depend on the directory ``ljos mcp`` started in.

``ljos_recall`` adds a Code context section when ``graphify-out/graph.json`` is in the working directory's repository.
The section is the same one ``ljos recall`` prints.
``LJOS_GRAPHIFY=off`` leaves it out. ``LJOS_GRAPHIFY`` set to a path reads that file.

``ljos_search`` keeps this seat's hits first. Another seat's hit is kept when it names the subject, or when the encoder's cosine with the query is at least 0.66. When nothing matched, the reading is ``Nothing matched "QUERY"`` and names a shorter query, in plain text, when one content word is shorter than the query. When nothing is stored, it names ``ljos remember "LESSON"``, because a shorter search would miss too. When the only rows were weaker, the reading is ``Nothing relevant was found.`` Pass ``project`` to keep that project's tickets and lessons, and lessons that name no project. An unknown project name is an error that names the projects that exist. ``ljos_remember`` takes ``project`` and records that project on the lesson.

``ljos_search`` and ``ljos_recall`` append one line to ``recall.log`` in the ljos state directory, the same line ``ljos search`` and ``ljos recall`` append.
The line is the time, the seat, ``search`` or ``recall``, and the query or the issue.
The seat is the one the call passed, else the one the session or the environment names.
The verb on the line is ``search`` or ``recall``.

``ljos_start`` takes an optional ``items`` list. The ``task`` is the parent title.
One call files that parent and the items in one tracker commit, claims the
parent, and returns the checklist with ids. Pass ``issue`` with ``items`` to
file the items under that ticket instead. ``ljos_sitting`` takes the same
``items`` list, and ``plan`` when the parent does not exist yet. ``ljos_finish``
takes ``issue``, the checklist id, and ``lesson``. It marks that id done and names the next item.
