:description: The tools the Model Context Protocol (MCP) server ljos-mcp serves, the core set and how to list every tool.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=========
MCP tools
=========


The seat serves these over the Model Context Protocol (MCP).

Writers: ``ljos_sitting``, ``ljos_finish``, ``ljos_calibrate``, ``ljos_persona``, ``ljos_playbook``, ``ljos_remember``, ``ljos_prefer``, ``ljos_forget``, ``ljos_trust``,
``ljos_learn``, ``ljos_graded``, ``ljos_island``, ``ljos_deed``, ``ljos_vote``, ``ljos_predict``, ``ljos_rule``, ``ljos_claim``,
``ljos_release``, ``ljos_complete``, ``ljos_consolidate``, ``ljos_findings`` (a reader without ``remember``), ``ljos_bump_plan`` (a reader with ``dry_run``), ``ljos_handover``, ``ljos_receive``, ``ljos_send``, ``ljos_read``, ``ljos_reply``, ``ljos_group``. Readers: ``ljos_search`` (with ``as_of``), ``ljos_conflicts``, ``ljos_inbox``,
``ljos_timeline``, ``ljos_digest``, ``ljos_due``, ``ljos_habit`` (a writer with a value), ``ljos_evidence``, ``ljos_current``, ``ljos_recall``,
``ljos_consensus``, ``ljos_brief``, ``ljos_personas``, ``ljos_playbooks``, ``ljos_cards``, ``ljos_policy``, ``ljos_doctor``. Resources:
``ljos://protocol``, ``ljos://cards/USER.md``, ``ljos://cards/MEMORY.md``. Prompts:
``start_a_sitting``, ``run_a_panel``, ``check_a_handover``. Every tool description opens with
when to call it.

By default the server lists the core set, in this order: ``ljos_rule``,
``ljos_prefer``, ``ljos_remember``, ``ljos_search``,
``ljos_start``, ``ljos_guard_check``, ``ljos_file``, ``ljos_options``, ``ljos_note``, ``ljos_recall``,
``ljos_sitting``, ``ljos_finish``, ``ljos_claim``, ``ljos_release``, ``ljos_send``,
``ljos_inbox``, ``ljos_reply``, ``ljos_vote``, ``ljos_consensus`` and ``ljos_doctor``.
``LJOS_MCP_TOOLS=all`` lists every tool above: the core set first, then the rest
by name, then the persona and playbook tools.
``LJOS_MCP_TOOLS`` also takes names split on commas or spaces, with or
without the ``ljos_`` prefix, where ``core`` stands for the core set. A call
to a tool the server does not list is refused with the shell command to
run instead. A tool that fails answers with ``isError`` set and the reason
as text, so the model reads it as a tool result.

``ljos_start`` takes an optional ``items`` list. The ``task`` is the parent title.
One call files that parent and the items in one tracker commit, claims the
parent, and returns the checklist with ids. Pass ``issue`` with ``items`` to
file the items under that ticket instead. ``ljos_sitting`` takes the same
``items`` list, and ``plan`` when the parent does not exist yet. A call that
sends no conversation id still sits, bound to this seat, the same sitting
``ljos sitting`` takes when ``LJOS_SEAT`` is set and no conversation is
stamped. ``_meta.conversation_id`` sits as that conversation. ``ljos_claim``
uses the same holder. ``ljos_finish``
on a checklist id marks it done and names the next item.

``ljos_search`` keeps this seat's hits first. Another seat's hit is kept
when it names the subject, or when the encoder's cosine is at least 0.66.
When the only rows were weaker, the reading is ``Nothing relevant was found.``
