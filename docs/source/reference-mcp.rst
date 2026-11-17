:description: The tools the Model Context Protocol (MCP) server ljos-mcp serves, the core set and how to list every tool.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=========
MCP tools
=========


The seat serves these over the Model Context Protocol (MCP).

Writers: ``ljos_sitting``, ``ljos_finish``, ``ljos_calibrate``, ``ljos_persona``, ``ljos_playbook``, ``ljos_remember``, ``ljos_prefer``, ``ljos_forget``, ``ljos_trust``,
``ljos_learn``, ``ljos_graded``, ``ljos_island``, ``ljos_deed``, ``ljos_vote``, ``ljos_predict``, ``ljos_rule``, ``ljos_claim``,
``ljos_release``, ``ljos_complete``, ``ljos_consolidate``, ``ljos_findings`` (a reader without ``remember``), ``ljos_bump_plan`` (a reader with ``dry_run``), ``ljos_handover``, ``ljos_receive``, ``ljos_send``, ``ljos_read``, ``ljos_reply``, ``ljos_group``. Readers: ``ljos_search`` (with ``query`` and optional ``as_of``), ``ljos_conflicts``, ``ljos_inbox``,
``ljos_timeline``, ``ljos_digest``, ``ljos_due``, ``ljos_habit`` (a writer with a value), ``ljos_evidence``, ``ljos_current``, ``ljos_recall`` (with ``issue``), ``ljos_ready``,
``ljos_consensus``, ``ljos_brief``, ``ljos_personas``, ``ljos_playbooks``, ``ljos_cards``, ``ljos_policy``, ``ljos_doctor``, ``ljos_mail_search``, ``ljos_mail_show``. Resources:
``ljos://protocol``, ``ljos://cards/USER.md``, ``ljos://cards/MEMORY.md``. Prompts:
``start_a_sitting``, ``run_a_panel``, ``check_a_handover``. Every tool description opens with
when to call it.

By default the server lists the core set, in this order: ``ljos_rule``,
``ljos_prefer``, ``ljos_remember``, ``ljos_search``,
``ljos_start``, ``ljos_guard_check``, ``ljos_file``, ``ljos_options``, ``ljos_note``, ``ljos_recall``, ``ljos_show``, ``ljos_ready``,
``ljos_sitting``, ``ljos_finish``, ``ljos_claim``, ``ljos_release``, ``ljos_send``,
``ljos_inbox``, ``ljos_reply``, ``ljos_vote``, ``ljos_consensus`` and ``ljos_doctor``.
``LJOS_MCP_TOOLS=all`` lists every tool above: the core set first, then the rest
by name, then the persona and playbook tools.
``LJOS_MCP_TOOLS`` also takes names split on commas or spaces, with or
without the ``ljos_`` prefix, where ``core`` stands for the core set. A call
to a tool the server does not list is refused with the shell command to
run instead. A tool that fails answers with ``isError`` set and the reason
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
A write with no seat names ``ljos seat`` and explains that ``LJOS_SEAT`` applies to reads only, so writes on a shared ljos mcp server pass ``seat`` in arguments or carry a stamped runner session.
``ljos_vote`` takes any option from the issue's ``--options`` in ``choice``, such as ``keep``, ``drop``, ``accept``, or ``reject``.
A busy pack names ``ljos search`` or ``ljos remember``.
An unknown tool names ``ljos help``.
An unknown method, or a line that is not JSON, names ``ljos mcp --help``.
``ljos_show`` prints the full ticket page ``ljos show`` prints: the header with the wrapped title, the claim if any, parent and children, blockers, the full body, and the latest notes.

``ljos_ready`` prints the same first page as ``ljos ready``. The default is 20 tickets. ``limit`` changes that count and ``all`` prints every ticket. The last line of a short page names ``--limit`` and ``--all``. This seat's claims come first, then unclaimed work by priority and recent activity, then other seats' claims, with long-stale claims last. The id breaks the final tie.

``ljos_doctor`` returns one row for each habitat with ``name``, ``state``, ``ok`` and ``optional``. A row with ``optional`` true is an extra the seat runs without, such as ``packset-embed``, ``ljos-hud``, the cloud guard, the runners or the git hooks, so ``ok`` false there is not a failure. It is the row ``ljos doctor`` prints as ``info``. A row with ``optional`` false and ``ok`` false needs a step. The install command a row names does not depend on the directory ``ljos mcp`` started in.

``ljos_recall`` adds a Code context section when ``graphify-out/graph.json`` is in the working directory's repository.
The section is the same one ``ljos recall`` prints.
``LJOS_GRAPHIFY=off`` leaves it out. ``LJOS_GRAPHIFY`` set to a path reads that file.

``ljos_start`` takes an optional ``items`` list. The ``task`` is the parent title.
One call files that parent and the items in one tracker commit, claims the
parent, and returns the checklist with ids. Pass ``issue`` with ``items`` to
file the items under that ticket instead. ``ljos_sitting`` takes the same
``items`` list, and ``plan`` when the parent does not exist yet. A call that
sends no conversation id still sits, bound to this seat.
That holder is the same sitting ``ljos sitting`` takes when ``LJOS_SEAT``
is set and no conversation is named. ``_meta.conversation_id`` sits as that conversation. ``ljos_claim``
uses the same holder. ``ljos_finish`` takes ``issue``, the checklist id, and ``lesson``.
It marks that id done and names the next item. ``node`` is not a field.

``ljos_search`` keeps this seat's hits first. Another seat's hit is kept
when it names the subject, or when the encoder's cosine is at least 0.66.
When nothing matched, the reading is ``Nothing matched "which fusion"``. It names a shorter query, in plain text, such as ``ljos search fusion``, when one content word is shorter than the query. When nothing is stored, it names ``ljos remember "kiln"`` for that one word, because a shorter search would miss too. When the only rows were weaker, the reading is ``Nothing relevant was found.``

``ljos_search`` and ``ljos_recall`` append one line to ``recall.log`` in the ljos state directory, the same line ``ljos search`` and ``ljos recall`` append.
The line is the time, the seat, ``search`` or ``recall``, and the query or the issue.
The seat is the one the call passed, else the one the session or the environment names.
The verb on the line is ``search`` or ``recall``.

