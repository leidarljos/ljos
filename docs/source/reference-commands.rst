:description: Every ljos verb, the stores it reads and writes, and what it does.
:cover: https://leidarljos.github.io/ljos/_static/og.png

========
Commands
========

.. raw:: html

   <span id="command-line"></span>

A command exits 0 when it did what was asked.
A bad command, a bad flag, or a bad flag value exits 2.
A runtime failure exits 1.
That is a missing program, a ticket the tracker does not hold, a store that is missing, read-only or full, a pack that was turned off, or a network error while upgrading.
Mail, inbox and recall exit 75 when the pack is busy.
A search that cannot ask the pack ranks by words from local data and says it fell back.
The first line says why. A writer that is not running names ``packset ensure``. A writer that stays busy says the rows are word matches. When no words match, search exits 1.
A remember that queued the lesson exits 0.
Every failure is one line on stderr.
The line says what went wrong and the command that fixes it.
A word that is not a command names the verb it was reaching for as a line that runs: a mistyped name, a name with an ending such as ``searches``, the start of one verb such as ``prop``, or a name with a part that is a verb such as ``search-lessons``. A start that two verbs share is not guessed. ``lessons``, ``lesson`` and ``learnings`` are not commands, and the line names ``ljos search`` to find a lesson, ``ljos remember`` to save one and ``ljos proposals`` to review the ones waiting. Words after ``lessons`` become the query of the search line.
``-p`` and ``--project`` name a project on ``calibrate``, ``claims``, ``file``, ``list``, ``ready``, ``remember``, ``search`` and ``sitting --plan``. On another verb the error names those verbs.
``LJOS_DEBUG=1`` or ``--verbose`` prints the cause chain after that line.

``sitting ISSUE [--assignee NAME] [--cards DIR] [--playbook NAME] [--anyway] [--verbose] [--json]``
   Stores: all. Open a sitting: doctor, sync when the repository names a scope, cards, due, island, blockers, playbook, workflow when the issue carries one, panel when the issue is a decision, recall, timeline, reclaim, claim. The short form prints what needs action, what the seat remembers, and the claim. A degraded pack is one line, ``matched by words (encoder not installed)``. A pack that answers 503 or does not answer is ``These are word matches; the pack did not answer.``, and the sitting continues. That search waits at most fifteen seconds when the encoder is cold. Quiet claims are a count that points at ``ljos release --stale``. Claims untouched for longer than ``LJOS_STALE_DAYS`` (7) are one line, ``N claims untouched for 7+ days: ljos claims --stale``. ``--verbose`` prints every section, including the tracker and sync lines. A commit that landed is not printed, and neither is a push that landed or a sealed log. A refused commit, a refused push, or a push that was skipped is one sentence that names the command to run, and that sentence does not name a host directory or a segment file. ``--json`` prints ``text``, ``tracker_git`` and ``sync``. Reclaim hands back claims quiet longer than ``LJOS_LEASE_SECS`` (900; 0 is off) whose conversation is gone, and keeps one whose runner still runs. It stops at the first store down. The name defaults to this conversation's holder (``ljos seat``). ``--playbook`` copies a recipe before recall; absent, title-match else ``sit``. The name is a tracker ``playbook:`` note until finish or release. ``--cards`` is the directory of ``USER.md`` and ``MEMORY.md``; ``--dir`` is the same flag. Example: ``ljos sitting proj-1a2b``.
``sitting --plan TITLE [--top] [-p PROJECT] -- ITEM...``
   Stores: tracker, claim graph. File ``TITLE`` and the items after ``--`` in one tracker commit, claim the parent, and print the checklist with ids. The parent is tagged ``plan`` and ``ORDERED``. Each item is a ``checklist`` child, in the order given. The sitting names the next unfinished item. ``ljos ready --mine`` names it again. ``ljos finish ITEM`` marks that item done and names the next one. ``-p`` without ``--plan`` exits 2 and names the verbs that take ``-p``. With no tracker set up, ``--plan`` exits 1, names ``ljos onboard``, and writes no ticket and no tracker directory.
``finish [ISSUE] [--status done|failed|cancelled] [--lesson TEXT] [--outcome OPTION] [--new-option] [--beta B] [--gen G] [--assignee NAME] [--close] [--quiet] [--verbose] [--json]``
   Stores: all. Close a sitting: mark the ticket finished, store ``--lesson``, then learn. A lesson written after the issue, with no ``--lesson``, is taken the same way. ``--verbose`` says whether related memories were linked, and prints the tracker and sync lines. A commit that landed is not printed, and neither is a push that landed or a sealed log. A refused commit, a refused push, or a push that was skipped is one sentence that names the command to run. ``--json`` prints ``text``, ``tracker_git`` and ``sync``. A checklist item is marked done instead, and the line names the next unfinished item of its plan. The lesson goes in at once at L0 (``derived``, a memory a person has not accepted) with origin ``agent-derived``. It is the person's when no runner marks the environment and a terminal is on its input; ``ljos accept ID`` raises it to ``explicit``; ``--new-option`` says the outcome was none of the ballot's options; ``--gen`` names the claim's generation (absent, the live one; a stale one is refused); ``--assignee`` names the holder (absent, this conversation's holder); a runner is refused. A person closes the ticket when the work is accepted, else the ticket stays open with no holder. With no ``--lesson``, a ticket that is still TODO and that nobody has claimed is left as it is, and the line names ``ljos sitting ISSUE``. Nothing is written. A lesson passed to that finish is still stored. With no id, ``ljos finish --lesson "..."`` finishes the issue this conversation holds. With none open, it exits 1 and names ``ljos sitting ISSUE``. With ``--close`` it names ``ljos finish ID --close`` instead. An empty id exits 1 and writes nothing. A named id that is not that issue is still finished. stderr names that id and the open sitting, ``finished ID; your open sitting is OPEN``. ``--quiet`` leaves that line off. Another conversation of this seat is not the open sitting. ``ljos finish`` with no id finishes this conversation's sitting. A stored lesson whose reading score is past 14 names the saved id and the sentence to change. A short lesson, and a lesson whose hard words are identifiers, paths, code, SHAs or versions, prints no reading hint. Example: ``ljos finish --lesson "Cold metal seats."``.
``calibrate -p PROJECT [--rounds N]``
   Stores: consensus, pack. Estimate how often each voter has been right, from the project's past votes. The estimate is Dawid-Skene: it uses the ballots and no separate answer key. ``--rounds`` is how many expectation-maximisation passes to run.
``remember TEXT [-p PROJECT] [--as PERSONA] [--transient|--standing] [--verbose] [--valid-from DATE] [--valid-to DATE] [--json]``
   Stores: pack. One lesson, stored as written, origin ``user-declared``. ``-p PROJECT`` records that project on the lesson. An unknown project name exits 1 and names the projects that exist. As a persona, it carries that persona's entity and opens its next brief; typed by the person it is standing at once, and the hook shows it from the next prompt it bears on; run under an agent runner it is an episode labelled agent-derived (an agent wrote it, and a person has not accepted it), kept and not a refresher until a review promotes it; ``--transient`` or ``--standing`` picks one either way. A lesson of one or two sentences is one row. A longer text is stored as one claim per pair of sentences, and the command prints each stored id and its text. A sentence of more than 29 words that the person types is stored as written, and ``--verbose`` prints one line to split it into two sentences. The same sentence from anywhere else is kept on this machine only: the command says the pack refused it, ``ljos search`` still finds the copy, and the hook does not show it. ``--valid-from`` and ``--valid-to`` are the days the claim is true in the world. A reading score past 14 is a hint, not a failure. The hint is printed only when the lesson, once identifiers, paths, code spans, commit SHAs and version strings are taken out, has at least 20 words and a sentence of at least 20 words. It says the lesson was saved, names that sentence, and gives the score and the aim of 14. ``--json`` prints the stored lesson with ``grade``, ``target`` and ``flagged_sentence``. A 503, including memory pressure, queues the lesson and a later command sends it. A duplicate prints the atom id already in the pack. A content refusal names size or policy. An empty text exits 2. A text past 1000 characters exits 2. When the pack is not answering, the lesson is queued and the command exits 0. A busy pack is asked again for ``LJOS_SEARCH_BUSY_WAIT`` seconds (2 when unset, ``0`` does not wait). The wait follows ``Retry-After`` or a JSON ``retry_ms``. When the pack stays busy, nothing was stored, the line says to run ``ljos remember`` again, and the command exits 1. Turned off on purpose, the command exits 1. A pipe prints the same lines as a terminal. Example: ``ljos remember "The lexical default is BM25+."``.
``memories list|review|tui|prune``
   Stores: pack. Review the memories agents wrote on their own: ``list`` newest first, shown N of N, each claim wrapped with its seat, its ticket and a short id; unreviewed agents' memories by default; ``review`` keeps, drops, edits or merges them; ``tui`` browses them; ``prune`` names duplicates and memories that were never recalled. ``--stale`` is required, and it writes only with ``--apply``. Example: ``ljos memories prune --stale``. ``list --json`` prints the rows.
``prefer TEXT [--deny PATTERN] [--ask PATTERN] [--user --why REASON]``
   Stores: pack, user config. One standing preference, origin ``user-declared``; each ``--deny`` or ``--ask`` also writes that rule with the preference as its reason, so the hook warns by it. ``--user`` writes a user rule instead, shared by every seat on this machine, never reviewed, and not also stored as a seat preference; ``--why`` is required with it. ``--as`` stores it as that persona. ``--promote`` copies a lesson into a user rule. ``--forget`` tombstones a user rule. ``--sync`` pulls and pushes user rules through the tracker. ``--review`` lists preferences stored before a basis was kept, and ``--keep`` keeps one. ``--deny-mention``, ``--deny-name``, ``--deny-reply-to`` and ``--deny-reply-name`` refuse an outbound post, with ``--user``. See :doc:`User rules <reference-rules>`
``accept ID [--quote WORDS --at TIME --via SEAT]``
   Stores: pack. Raise one stored memory from L0 (``derived``) to ``explicit``, or a proposal an older ljos left open, and record a ``keep`` review. Any caller can, and the memory keeps its origin. For a yes said in chat, ``--quote`` takes the person's words, ``--at`` the time they said them as a Request for Comments (RFC) 3339 timestamp, and ``--via`` the seat that carried them. All three go together, and the acceptance keeps them. A proposal whose text ``remember`` or ``prefer`` already wrote is not written again.
``forget ID [--why DEED] [--json]``
   Stores: pack. Tombstone a claim, naming the deed that withdrew it. ``--why`` is a deed id rather than a sentence. ``--json`` prints the stored row.
``retire-notes``
   Stores: pack. Tombstone the note memories an older ``ljos note`` wrote; runs once per seat, and the tracker copies stay.
``search QUERY [-p PROJECT] [-n N] [--rerank] [--as-of TIME] [--valid-at DATE] [--explain] [--source-machine NAME] [--source-seat SEAT] [--import-id ID] [--min-score SCORE] [--json] [--porcelain] [--attempts]``
   Stores: pack, tracker. ``-p PROJECT`` keeps that project's tickets and lessons, and lessons that name no project. An unknown project name exits 1 and names the projects that exist. Ranked claims with how many scorers named each and its age. This seat's hits come first when the encoder ranked them. A word ranking, which is what a local snapshot and an encoder that is not loaded use, scores by BM25 over words and split identifiers (``list_scans`` matches ``list scans``, ``SHA-1`` matches ``sha1``). packsetd with no encoder reports ``engine: linear``. Its own tokenizer still keeps ``list_scans`` as one word and scores a short prefix almost like an exact match, so a memory has to be among the rows it returns. Splitting ``_``, ``-``, ``.`` and a digit boundary in ``packset_core::search::tokens``, and not treating that prefix as an exact identifier, is the change on that side. A title or a tag counts twice a body word. Words that sit together get a proximity bonus. This seat's row gets a small boost, and a clearly closer row from another seat stays ahead. A row that does not cover enough of the query is left out. A word in every row is common only once at least four rows carry it, so two lessons that share one word still match. When every row is in that state the line is ``No close matches for "..."`` and it names a broader query. A row that does not name a subject word is left out when its score is under 34, and a row that only shares a stopword or a common weak word is left out. A score runs from 0 to 100. ``--min-score SCORE`` sets that line. ``--min-score 0`` prints every row. When the floor removes every row the line is ``No relevant matches for "QUERY".`` and it names ``--min-score 0``. A meaning row from another seat that only shares a generic word is left out unless the encoder's similarity is high. ``--json`` then sets ``close_matches`` to false and puts that sentence in ``reading``. ``ranking`` stays ``lexical``. The page names the place, the checks, a short id and the ticket. That id opens with ``ljos show``. A ``local:`` id opens the same way. The ranker score stays off that page. ``--json`` keeps the score. A tracker ticket whose title matches is included, and then one whose body or notes match. A title that shares only one common word with a longer query is left out. A distinctive word, or two shared words, still matches. An exact or near-exact title is the first row, while the encoder is loading and after it has answered. A ticket this seat filed, claimed, or noted, whose title, body or notes cover two query terms, is placed with this seat's rows. The newest note weighs more than an older one. A ticket in a project this seat works in comes before another project's ticket that only shares a generic word such as docs, release, or video. A ticket with no seat tag is not this seat's, unless the tracker roster names only this seat. That is the line ``This seat's rows come before another seat's.`` The row names the ticket id and the title, so ``ljos recall ID`` opens it. Another seat's hit is kept when it names the subject, or when the encoder's cosine with the query is at least 0.66. The cosine is how close the stored text is to the query, from 0 to 1. A short list is not padded with a weaker cross-seat hit. When those were the only rows, the line is ``Nothing relevant was found.`` No match-mode line is printed on that empty page. A search that found nothing prints no match-mode line. The line is ``Nothing matched "which fusion". Try a shorter query, such as: ljos search fusion``. The word is the longest content word, stop words skipped, and it is named only when it is not the whole query. The command is plain text, on its own line when the sentence would split it. When nothing is stored, a shorter search would miss too, and the line names ``ljos remember "LESSON"``. ``--json`` adds ``suggestion`` with the command the line names. A busy memory store is asked again for ``LJOS_SEARCH_BUSY_WAIT`` seconds (2 when unset, ``0`` does not wait). The wait follows ``Retry-After`` or a JSON ``retry_ms``. When it answers, the rows are ranked by meaning. When it stays busy, the rows are ranked by words and a hit that names the subject stays. The one line is ``The memory pack was busy, so these rows are word matches; run the search again for ranked results.`` A pack that accepts and does not answer uses that line when words match, after one short request. When no words match, the line is ``The memory pack was busy and no words matched.`` and the command exits 1. A pack that has not opened is waited out on ``LJOS_SEARCH_WARMUP_WAIT``. The signal is ``/health`` equal to ``not-ready``, or JSON ``ready`` false, or JSON ``code`` ``starting``. Words in the query are not that signal. When the wait runs out, search ranks by words and the one line is ``Used word search because memory is still starting.`` A dated search prints ``Memory is starting, so try again in a few seconds.`` and exits non-zero. A search while the encoder is still loading waits out a remaining time that fits in ``LJOS_SEARCH_WARMUP_WAIT`` seconds (5 when unset, ``0`` does not wait), and not past ``LJOS_CALL_TIMEOUT``, then ranks by meaning. A remaining time that is missing or longer than that budget keeps ``LJOS_SEARCH_WAIT`` (3 when unset). If it is still not ready and rows are shown, the line says they are word matches and reports the remaining load time when one is sent. When nothing is shown, one line names the command to rerun and, when the writer sends a remaining time, about how many seconds are left. A failed or missing encoder names the command that fixes it. ``--json`` puts ``semantic`` or ``lexical`` in ``ranking``. Starting the memory writer starts that load. ``--rerank`` scores the top hits again, slower and sharper, by reading the text itself; ``--as-of`` reads the memory store as it stood then, and a value that is not a real YYYY-MM-DD date or an RFC 3339 time exits 2; ``--valid-at`` asks what held in the world that day (``any`` keeps every window); ``--explain`` opens with how the rows are ordered and what the checks mean. ``--source-machine``, ``--source-seat`` and ``--import-id`` keep only those rows, before the limit. A shortened page opens with ``shown X of Y.`` and a quoted ``ljos search ... -n N`` command to list the rest. ``--json`` prints the rows. If the backend emits receipt tab rows, the formatted page still opens with ranked hits, and ``--json`` puts those rows in ``receipt_rows``. ``--porcelain`` prints one row per hit, the id, the kind and the text, separated by tabs, and leaves the page off. A receipt and another seat's mail stay off the page. ``--attempts`` includes failed commands. A memory store that does not answer ranks by words from what is stored locally, prints one line that names ``packset ensure``, and exits 0. The command appends one line to ``recall.log`` in the ljos state directory: the time, the seat, ``search`` and the query. ``ljos_search`` appends that same line. Example: ``ljos search "lexical default"``.
``timeline ISSUE [-n N] [--json]``
   Stores: tracker, deed store, pack. The logbook, the cited deeds and the activated memories as one dated list, oldest first. The page says shown N of M and names each event's age and the gap since the one before. ``--json`` prints the rows.
``consolidate [--apply]``
   Stores: pack. The pairs where a later claim rewrites an earlier one; ``--apply`` closes the earlier ones' windows.
``conflicts [-n N]``
   Stores: pack. Candidate contradictions by geometry: the lowest passes between single memories, read by the optional ``landscape`` habitat. The habitat is not part of this repository. When it is not on ``PATH`` the command names https://github.com/leidarljos/landscape as its source and writes nothing.
``hubs [-n N] [--json]``
   Stores: pack. The memories most other memories point at, highest first. The page names the score and how many memories point at the row. PageRank ranks a memory by how many others point at it, and by how linked those are. ``--json`` prints the rows.
``island CUE [--fire] [--as PERSONA] [--json]``
   Stores: pack. The memories a task activates: search hits spread along the links, strongest first. The page names the activation and whether the row is a seed. ``--fire`` wires the strongest eight together. ``--as`` walks the links as that persona. ``--json`` prints the rows.
``due [--all] [--judge] [--json]``
   Stores: pack. Sweeps the pack first (reviews left due past twice their interval lapse; never-recalled claims missed three times are forgotten), then the claims whose review is due, soonest first. The page says shown N of M and names the seat and the ticket. ``--all`` lists every due memory and grades none. ``--json`` prints the rows. ``--judge`` sends the page to the review judges.
``hud [--foreground] [--toggle|--show|--hide] [--install-desktop]``
   Stores: none. Exec sibling ``ljos-hud``. A missing program exits 127. ``--hide`` when that program is not installed exits 0. The musl CLI never links iced.
``habit [NAME [VALUE]] [--unit U] [--every 7d] [--source S]``
   Stores: pack. With a value, take a reading: the earlier reading closes and is kept as what it was, the next is due one cadence on; without, list each habit's latest reading with the change since the one before.
``graded ID [--lapsed]``
   Stores: pack. One review graded.
``trust FROM TO WEIGHT [--why DEED]... [--about DOMAIN]...``
   Stores: pack. One trust row; scoped when ``--about`` is given.
``persona NAME --anchor A --view TEXT [--about DOMAIN]... [--runner RUNNER] [--replace] [--check]``
   Stores: pack. A voter with a view; ``A`` in [0, 1] is how far it moves off its ballot; writes one unscoped inbound trust row; ``--runner`` names a ``[[harness]]`` that reasons as the persona in a session it keeps; a name another seat or conversation defined is refused unless ``--replace``; ``--check`` says what the define would do and which personas share its view or domains, and writes nothing.
``persona NAME --retire [--replace]``
   Stores: pack. Withdraws the persona, so it casts no more ballots; only the conversation that defined it may, unless ``--replace``.
``personas``
   Stores: pack. The roster: name, anchor, domains, view, one per line.
``agents [--harness NAME] [--dry-run]``
   Stores: pack, runner. Every persona as an agent definition, ``ljos-NAME.md``, in each runner's ``agents`` directory (or one runner's): a subagent the runner spawns by name that briefs, casts one ballot before reading the others, notes why and stops; definitions whose persona left the pack are removed.
``statusline``
   Stores: pack. One line for a runner's status bar (the build harness's ``[ui.status_line]``, the terminal harness's ``statusLine``): the seat, the issue this conversation has claimed, the claims due; reads the runner's status JSON on stdin; each session's line is cached for fifteen seconds.
``ask NAME TEXT``
   Stores: persona session. Hands the persona a question or a task in its own pane, opening it (and resuming its session) when it is closed.
``supervise NAME --harness H --exit clean|crash --at SECONDS``
   Stores: runner. Classify one harness exit and print ``restart``, ``stop`` or ``give-up``, and on a restart the herdr line that would start the child. It prints the line and does not run it. ``--restart`` is ``permanent`` (the default), ``transient`` or ``temporary``; three restarts inside sixty seconds give up.
``upgrade [VERSION] [--dir DIR] [--only NAME]``
   Stores: release. Installs the ljos release beside the running ``ljos``, then each other seat program from the repository that publishes it. The archive is checked against the sha256 published beside it, each binary is an executable that names the version, and the old one is kept as ``NAME.bak-pre-VERSION``. One line a program: ``NAME OLD NEW``, or ``NAME VERSION current``. A missing release, a failed download or a checksum that does not match is a warning, and the other programs still move. ``--only NAME`` moves that one program. ``VERSION`` is the ljos release. This is how an agent moves the seat's binaries, which policyd keeps it from writing.
``playbook ISSUE NAME``
   Stores: pack. Bind a recipe to the issue and copy its full body; the name is a tracker ``playbook:`` note until finish or release.
``playbooks``
   Stores: pack. The closed set sit, arena, land, company-panel, overnight, feature; pack latest per name, the built-in seed when missing.
``workflow ISSUE [--start NAME] [--round]``
   Stores: tracker. A parent's workflow: stage, round and next step; ``--start feature`` notes ``workflow: feature`` and files the design child, ``--round`` files the next panel round once every child is closed.
``brief NAME ISSUE``
   Stores: pack, tracker. What a subagent playing the persona starts from: view, the bound playbook's full recipe, five named principles, the arena rubric, what the seat knows on its domains, the working set.
``panel [run] ISSUE [--out DIR] [--jev] [--harness NAME] [--designers N] [--dry-run] [--cloud]``
   Stores: pack, tracker. One brief per persona whose domains the issue speaks to as ``DIR/<name>.md``, then the settle line; refused until a playbook is bound. ``--jev`` asks Jev for every ballot first and casts them only when every seat is sure and all agree; otherwise it writes the briefs. ``--harness`` names the runner that spawns the panel. ``--designers`` is how many designers ``run`` spawns. ``--dry-run`` prints the sessions and does not spawn. ``--cloud`` writes one prompt file per persona and does not spawn.
``open-panel --prompt-file FILE --log FILE [--cwd DIR] [--session ID]``
   Stores: tracker, runner. Open a panel for a decision prompt: file or reuse the issue, sit it, and start one headless member per brief. The prompt is already in the file, so the hook can return. ``--session`` keeps a second opener from starting a second panel.
``learn ID --outcome OPTION [--rule record|hedge] [--beta B] [--share S]``
   Stores: tracker, pack. Reweigh voters by what turned out right: by default each voter's record of hits and misses as log-odds weights, its accuracy shrunk toward the panel's pooled accuracy; ``hedge`` shrinks refuted voters by ``B`` with ``S`` recovery; either way the outcome is kept. ``--new-option`` says the outcome was none of the ballot's options.
``evidence ACCESSION`` / ``evidence record -- COMMAND`` / ``current ACCESSION``
   Stores: deed store. Intact; ``record -- COMMAND`` runs the repository's test command and stores a passing record for the current tip; ``current`` checks whether a stored file is still the tip.
``record check DIR`` / ``record check --tracker ROOT``
   Stores: none / tracker. ``DIR`` holds schema-v1 question and ballot entries; ``--tracker`` holds every ballot under ``ROOT``, a persona's and a judge's included, to one test: the voter's newest ``ballot-sig:`` note names the choice, verifies, and is under the key ``.ljos-seat-keys`` pins for that voter and no other seat. One ``FILE:LINE: ISSUE: REASON`` line per ballot that does not stand, and exits 1. An unsigned ballot committed before signing began is legacy. It only reads, so CI can run it.
``file TITLE [--parent ID|--top] [-p PROJECT] [-t TYPE] [--tags T] [--priority P] [--body TEXT] [--options A,B] [--quorum N --deadline RFC3339] [-q] [--verbose] [--json]``
   Stores: tracker. File work found while sitting: a child of the held issue unless ``--parent`` names another or ``--top`` none, in the parent's project; a projected board takes it into its inbox under the printed id; the tracker file is committed. ``--options`` writes ``Options: A, B`` as the first body line. ``--quorum`` and ``--deadline`` together say how many ballots are due, and when. ``-q`` (``--porcelain``) is accepted and changes nothing: the id is already the only line on stdout. A commit that landed is not printed, and neither is a push that landed. A refused commit, a refused push, or a push that was skipped is one sentence that names the command to run. ``--verbose`` prints the tracker and sync lines. ``--json`` prints ``id``, ``text``, ``tracker_git`` and ``sync``. A day's checklist is ``ljos sitting --plan``, which files the parent and its items in one commit. ``-p`` is one name: a leading segment that repeats the tracker directory or its org prefix is dropped, and any other slash is refused, so an id never contains ``/``. A held issue the tracker does not have is dropped, and the ticket is filed at the top of the project. With no held issue, no ``--parent``, no ``-p``, and no project in checkout config or ``LJOS_PROJECT``, ``ljos file`` exits 1 and names ``-p NAME``. Example: ``LJOS_SEAT=alice ljos file "Pick a codec" -p demo``.
``ready [-p PROJECT] [-n N | --limit N | --all] [--json] [--mine]``
   Stores: tracker. Tickets that are open and not waiting on an open ticket. This seat's claims come first. Unclaimed tickets follow by priority, then by the newest note, claim or filing. Another seat's claims follow, with claims untouched for ``LJOS_STALE_DAYS`` (7 when unset) last. The id breaks the last tie, so the order is deterministic. The page prints the first 20, headed ``ready  shown X of Y``. ``-n N`` and ``--limit N`` print N. ``--all`` prints every ticket. A short page ends with one line naming ``--limit``, ``--all``, ``-p`` and ``--mine``. ``--json`` prints ``shown``, ``total`` and ``rows`` for that same page and order. ``--mine`` keeps its separate meaning: it names the next unfinished item of each of this seat's plans. When the open sitting's ticket is not a plan, the line names that ticket and says it has no plan items. ``-p`` limits the list to tickets in one project.
``list [-p PROJECT] [-n N | --limit N | --all] [--json] [--mine]``
   Stores: tracker. The same command as ``ready``, with the same flags, the same order and the same page, headed ``ready  shown X of Y``. ``ls`` is the short spelling. ``-n N`` prints N tickets, ``--limit N`` is the same flag, and ``--all`` prints every ticket. ``-p PROJECT`` limits the list to tickets in one project. ``--json`` prints ``shown``, ``total`` and ``rows``. ``--mine`` names the next unfinished item of each of this seat's plans. When the open sitting's ticket is not a plan, the line names that ticket and says it has no plan items. The command exists so a new seat that types ``ljos list`` gets the open tickets. ``ljos issues``, ``ljos tickets`` and ``ljos todo`` are not commands, and each names ``ljos ready``.
``show ID [--json] [--all | --verbose]``
   Stores: tracker, pack. One ticket, or one memory. A ticket id prints the full ticket page: its id, status, priority and full title wrapped to 80 columns, the claim seat and age when claimed, its parent and children, the tickets it waits on, the full body, and the notes. An open blocker is ``open`` and a finished one is ``closed``. None prints ``parent: none``, ``children: none`` and ``blockers: none``. The page prints the eight most recent notes, newest last. Each note has its time, age and the seat that wrote it. A longer log prints how many notes are earlier and names ``ljos show ISSUE --all``. ``--all`` prints every note. ``--verbose`` is the same flag. A memory id prints that memory. The id may be the full atom id, the eight hex characters ``ljos search`` and ``ljos remember`` print, or the ``local:`` form. The text wraps to 80 columns. The page names the kind, the level, the origin, the seat and the session that wrote it, when it was written, whether it is accepted, retired or superseded and by what, the tickets it cites, and the memories it links to. ``--json`` prints the ticket, including ``claim``, ``parent``, ``children``, ``blocked_by``, ``body`` and ``notes``, or the memory. An unknown id exits 1 and says it is neither a ticket nor a memory, and it names ``ljos search`` and ``ljos ready``. An id that matches more than one memory lists those ids. An id that matches more than one ticket exits 1 and names one ``ljos show`` command. A ticket filed under a doubled project path is found by the id in the file (``Software/hle-mm-yled``) and by that id with the leading segment removed (``hle-mm-yled``).
``note [ID] TEXT... [--quiet] [--verbose] [--json]``
   Stores: tracker. A dated progress note on the issue, committed. With no id, ``ljos note "TEXT"`` writes on the issue this conversation is working on. With none open, it exits 1 and names ``ljos sitting ISSUE``. An empty id exits 1 and writes nothing. ``ljos note ID TEXT`` notes on ID. When ID is not that issue, the note is still written and stderr names both; ``--quiet`` leaves that line off. On an issue another seat claimed (including across separate home directories sharing one tracker) it says so: the note is an assist, and ``ljos send`` reaches the holder; the note stays on the ticket and is never written into memory. A commit that landed is not printed, and neither is a push that landed. A refused commit, a refused push, or a push that was skipped is one sentence that names the command to run. ``--verbose`` prints the tracker and sync lines. ``--json`` prints ``text``, ``tracker_git`` and ``sync``. Example: ``ljos note "the door seats when it is cold"``.
``withdraw ID [--note TEXT] [--as NAME]``
   Stores: tracker. Take back this seat's own ballot, or with ``--note`` a note it wrote; nothing is deleted, and the withdrawal is a dated note beside it; ``--as`` names the persona a ballot was cast under.
``reopen ID``
   Stores: tracker. Put a finished or cancelled issue back to the state the close took it from, whoever closed it; a person has to be present; the close stays on the logbook with the reopen under it, and the caller does not take the claim.
``log --seat SEAT``
   Stores: tracker. List one seat's tracker commits. The subject is ``chore(issues): ID VERB by SEAT``.
``usage --since YYYY-MM-DD``
   Stores: tracker. How many tracker commits each seat made from that day on.
``digest [since TIME or last] [--json]``
   Stores: tracker, deed, pack. One view of what each seat did, newest first: tickets filed, claimed and closed, votes, consensus, deeds, claims, guard warnings and rewrites, and memory. last continues from the previous run. ``--mark`` moves that marker. JSON does not, unless ``--mark`` is set.
``send SEAT TEXT... [--group NAME] [--interrupt] [--issue ID] [--new-seat]``
   Stores: pack. Write to one seat, or with ``--group`` to the other members of that group; ``--interrupt`` leads the next prompt; ``--issue`` threads the message on that vissue issue and the atom takes the issue's scope, so ``ljos sync`` carries it in that repository's sealed log.
``inbox [--all] [--sync [COMMIT]]``
   Stores: pack. Mail this seat has not read, then receipts on what it sent that it has not been shown. Each unread message shown is marked read, and a receipt is written. ``ljos hook --prompt`` writes a receipt for each message it shows. A memory store that does not answer is an error and the exit is non-zero, and so is a writer that refuses kind ``message``. A memory store that runs out of time prints ``pack busy, try again`` and exits 75. The pack is this seat's memory store. ``--all`` includes mail already read. ``--sync`` fetches the tracker and imports its sealed logs before listing.
``read ID``
   Stores: pack. Write a read receipt for one message; a second read writes nothing.
``reply ID TEXT... [--interrupt]``
   Stores: pack. Answer the sender, keep the issue, and carry ``reply:ID``.
``group NAME [--add SEAT]... [--remove SEAT]...``
   Stores: pack. List a group, or add and drop members; membership is atoms and travels in the sealed log.
``wake [SEAT] [--command CMD [--timeout S]|--webhook URL [--key-file F] [--key-env VAR]|--clear|--fire]``
   Stores: machine. How mail wakes a seat that is idle: a shell line this machine runs with ``LJOS_SEAT`` set to the seat, or a URL it posts the envelope to (never the body); ``send`` and ``reply`` start it in its own session and do not wait on it; policyd reads a command when it is set and on every run, and a rule that fires is a warning beside it while the command still runs, and block-on-warning warns with that label and the command runs. It does not block; ``--fire`` runs it now and prints what happened. With ``--fire``, ``--from``, ``--msg``, ``--issue`` and ``--interrupt`` name the message. Alone, show the wake. Kept in ``~/.config/ljos/wake.toml``.
``deed ID [--add ACCESSION]... [-q|--porcelain]`` / ``--remove ACCESSION --ticket ID``
   Stores: tracker. Cite with ``--add``, or list; ``--remove`` drops one citation, appended to the ledger, and the heading is not rewritten.
``claims [--mine | --stale [--days N] [--json]] [ARGS]``
   Stores: tracker. Who has claimed what: ``vissue claims`` with its flags (``--by NAME``, ``--json``, ``-p PROJECT``). ``--mine`` lists this seat's claims with no note, stored file or finish for a day. ``--stale`` lists the ones untouched for ``--days`` or longer (``LJOS_STALE_DAYS``, 7 when unset), oldest first, with the age and the command to finish, release or note each one. ``--json`` prints that list as an array.
``recall ID [--json] [--no-graph]``
   Stores: tracker. The working set: the plan, the cited files and the notes for one ticket. When no lesson is stored on the issue, the line names ``ljos remember``. A 32-hex tracker id is accepted. Free text or an argument without a ticket id exits 2 and points at ``ljos search``. Lessons matched by the title or the issue's tags, and not filed on the issue, are listed under Related, with shown N of M. An id the tracker does not hold exits 1 and names ``ljos ready``. When ``graphify-out/graph.json`` is in the repository, a Code context section lists matching nodes with file and line and a few edges tagged EXTRACTED or INFERRED. ``--no-graph`` and ``LJOS_GRAPHIFY=off`` leave it out. ``--json`` puts the same text in the JSON document. A schema major other than 1 is named in that section, and the working set still prints. When nothing is storing memories, the command exits 75 when the line names ``packset ensure``, and exits 1 when packset is not on PATH. The command appends one line to ``recall.log`` in the ljos state directory: the time, the seat, ``recall`` and the issue. ``ljos_recall`` appends that same line. Example: ``ljos recall proj-1a2b``.
``vote ID [--for OPTION] [--confidence P] [--used ACCESSIONS|none] [--expect OPTION|JSON] [--as PERSONA] [--jev] [--withdraw]``
   Stores: tracker. Cast as this seat, never another, or read the tally; ``--confidence`` is a probability in (0, 1]; ``--used`` lists the deed accessions the ballot used, comma-separated, or ``none``, and is required with ``--for``; ``--expect`` records the forecast of the others on the same command; ``--withdraw`` takes back the ballot and its forecast, and the logbook keeps what it was. ``--as NAME --jev`` asks Jev for the persona's ballot: cast with the option's probability and a forecast at confidence 0.8 or above, noted and left for a subagent under it. ``--against REASON`` votes no, and ``--child ID`` names the open child that no ballot links. The tracker records the voter as ``VISSUE_AGENT``, which is this seat's name.
``predict ID --expect OPTION|JSON [--as PERSONA]``
   Stores: pack. Forecast the others' shares; two or more and ``consensus`` names the answer more popular than forecast (Prelec, doi:10.1038/nature21054)
``rule PATTERN [--verdict deny|ask|allow] --why TEXT [--user] [--json]``
   Stores: pack, user config. Argv law the hook and ``policy`` warn by, the verdict kept as a label: a glob, or a regular expression (``re:`` or a ``\b``, ``\s`` or ``(a|b)`` in it) anchored at the start, tried on the whole line and on each command it runs; :doc:`What a rule reads <reference-rules>` lists which those are. ``--why`` is the reason the warning gives, a sentence rather than a deed id. ``--user`` writes the law into the user config instead of the seat pack, shared by every seat and never reviewed. ``allow`` is kept only with ``--user``. ``--json`` prints the stored row.
``rules [--json]``
   Stores: pack. The rules the hook and ``policy`` warn by: verdict, pattern and reason, deny first. ``--json`` prints pattern, verdict and reason.
``consensus ID [--compare]``
   Stores: consensus, tracker. First what the seat ballots alone decide for the push check; then the settle under the pack's rows that apply to the issue, with every persona's anchor, as advice; an issue tagged ``broad`` runs bounded confidence; once six issues have an outcome, voters shown to err together are discounted; ``--compare`` adds the tracker's own settle, and so does a missing model crate; an issue with no ballots is refused before a settle runs; ``--json`` prints one JSON document, the seat tally at the top level and each reading under its own key. ``--no-file`` reads only and files nothing. ``--quorum N`` exits 1 when fewer ballots are in: nothing is decided yet. With ``--deadline``, still short after that RFC 3339 time, the vote has lapsed and this exits 3.
``claim [ID] [--next] [--role ROLE] [--slack N] [--assignee NAME]`` / ``claim ID --takeover [--assignee NAME]``
   Stores: claim graph, tracker. Take a ticket so this seat is the one working on it. The tracker stores the claim as a session node, its record of who has the ticket. With ``--next``, or with no id, it claims the first ready node in claimdag's order for the name, the way ``claim ID`` claims one; ``--role`` and ``--slack`` set the affinity and the depth slack. A busy refusal names what the name has still claimed. When another seat has the ticket, the refusal says whether that holder looks stale: the claim has been quiet for at least ``LJOS_LEASE_SECS`` (900 seconds when unset) and no running conversation on this host is named for it. ``--takeover`` takes a stale claim over: it frees the old holder's node in the claim graph, moves the tracker claim to this name, and adds a note on the ticket naming the old holder. A holder that is running, or whose claim moved inside the lease, is refused with a nonzero exit and nothing changes, and ``LJOS_LEASE_SECS=0`` turns ``--takeover`` off. ``--takeover`` needs an id and does not go with ``--next``. The ``ljos_claim`` tool takes the same step with ``takeover`` set to true through ``ljos mcp``. Example: ``LJOS_SEAT=alice ljos claim demo-2f5j``.
``release ID [--assignee NAME]`` / ``release --stale [--older-than DURATION] [--dry-run] [--verbose]`` / ``release --stale [--days N] [--yes]``
   Stores: claim graph, tracker. Hand one ticket back. It stays open, and the claim generation moves. A node another conversation of this seat holds is released under that name. ``--stale`` releases every claim this seat holds that has been quiet longer than ``--older-than`` (``LJOS_LEASE_SECS``, 900 seconds when unset) and whose conversation is not running on this host. Another seat's claim stays. The ticket stays open. ``--dry-run`` prints the count and changes nothing. ``--verbose`` prints one line per released ticket. ``--days`` lists claims untouched for that many days and writes nothing. ``--yes`` hands those back. ``--stale`` with an id is refused.
``reap [--idle 6h] [--apply]``
   Stores: claim graph. Release claims idle past a window and mail each seat. Without ``--apply`` it prints what would happen. ``--idle`` is how long a claim may sit with no claim or tracker commit: ``6h``, ``90m``, ``2d``.
``complete ID [--status done|failed|cancelled] [--gen G] [--assignee NAME]``
   Stores: claim graph. Finish the session node; does not close the ticket; ``--gen`` and ``--assignee`` as on ``finish``; a node another conversation of this seat holds is completed under that name, and another seat's is refused.
``cards [--dir DIR]``
   Stores: cards. Print ``USER.md`` and ``MEMORY.md``. ``--dir`` (alias ``--cards``, the same flag ``ljos sitting --cards`` uses) names the directory. With neither file, the line names the directory. Example: ``ljos cards``.
``policy [--fail-on-deny] ARGV...``
   Stores: policy, pack. The line as it would run, the trusted computing base (TCB) answer if ``ljos-policyd`` gave one (from 0.35.0 that may be a rewritten line), then a matching pack rule as a warning line, then what the pack knows that bears on it; ``--fail-on-deny`` exits 1 only on the cloud seat push rule, not on ``block-on-warning``, and without the flag the exit stays 0. Example: ``ljos policy -- git push --force``.
``hook [--limit N] [--prompt [--text]] [--fail-on-deny] [--runner NAME] [--git-commit] [--git-hook post-commit|post-rewrite]``
   Stores: policy, pack. Hook JSON or argv on stdin; ``--runner`` reads the payload of ``copilot``, ``gemini``, ``windsurf``, ``factory``, ``kiro``, ``cline``, ``qwen`` or ``crush`` and answers in that runner's shape; ``--prompt`` reads stdin as the prompt, and ``--text`` with it prints what the agent would read as plain text instead of JSON; on ``PreToolUse`` the TCB and pack rules, a rule is an allow with one warning line and block-on-warning warns with the rule's label and does not block, and from 0.35.0 a rewrite goes back as the updated input for the harnesses that accept one, including the editor harness's ``preToolUse``, and another runner runs its own line with a warning that names the new one; ``--fail-on-deny`` exits 1 only on the cloud seat push rule, not on ``block-on-warning``, and without the flag the exit stays 0; on a prompt the hits scoring at least 0.6 of the best that name its subject, or with a pack that sends cosines the encoder's first hit at 0.66 or more and a hit sharing a word within 0.05 of the best cosine, preferences first then lessons oldest first, each with its age, three at most, each once between compactions; on a failed tool, up to three that share at least two content words with the command and its error. ``--git-commit`` books the commit git just created and reads no stdin; ``--git-hook`` names ``post-commit`` or ``post-rewrite``. ``--event`` names the event when the payload does not. ``--brief-once`` prints the SessionStart brief once for this conversation. ``--drain`` applies bookkeeping a hook call queued. A pack that does not answer still exits 0, because a hook must not fail the runner. :doc:`Git hooks <reference-git-hooks>`
``git-hooks install|uninstall|status``
   Stores: git. Install, remove, or show the global git hooks that book commits. ``install`` prints ``installed`` and the directory; ``uninstall`` prints ``uninstalled``; ``status`` prints where the hooks are and what ``core.hooksPath`` says. :doc:`Git hooks <reference-git-hooks>`
``bump-plan BUNDLE --project P --parent I [--generation G] [--dry-run]``
   Stores: tracker. One child issue per module the bundle's lock builds, under ``I``, blocked by the modules built before it along the edges of its software bill of materials; ids are a hash of module and generation, so a rerun keeps what exists and adds what is missing; ``--dry-run`` prints the rows.
``findings STATE [--remember] [--all] [--issue I]``
   Stores: pack, deed store, tracker. The typed findings of an eb-stack campaign state, one per line: id, status, class/disposition, stage, recipe, the fix or the summary; ``--remember`` stores one lesson per finding a person or a seat resolved at once (recipe, step, error line, fix), origin ``tool-echo`` at L0 (``derived``), under the recipe's name, its package and the class; ``ljos accept ID`` raises it to ``explicit``; ``--all`` takes the ones a later attempt superseded too; ``--issue`` cites the state file as a deed on the issue.
``memory export --out FILE [--machine NAME] [--json]``
   Stores: pack, tracker. Write this seat's lessons to the file. The file is not sent. ``-`` writes it to stdout. The machine is ``--machine``, else ``LJOS_SYNC_HOST`` when that is set and not empty, else the hostname. Another seat's lessons stay out. The line is ``exported N memories to PATH``. When the lessons already in the file are the same, the file is left in place and the line is ``seed unchanged: N memories``. ``--json`` prints ``memories``, ``unchanged`` and ``path``. A busy pack is not waited out. A directory prints ``Error: memory: PATH is a directory. `ljos memory export --out FILE` writes a file`` and exits 1.
``memory import FILE [--source-machine NAME] [--source-seat SEAT] [--as-machine NAME] [--json]``
   Stores: tracker. Store the file in this seat's import slot under ``<tracker>/.ljos/memory/imports/<seat>/<sha256>/``. The import id is the SHA-256 of the file. ``--source-machine`` and ``--source-seat`` name the file when it carries neither. ``--as-machine`` records that machine and does not change the bytes. Rows are labelled ``imported,unintegrated``. One line names the counts: ``imported N, unchanged M, skipped K duplicates`` when a row repeats a content id in the file. A second import of the same bytes prints ``nothing new: N rows already present`` and does not call the memory store again. ``--json`` prints those counts and keeps ``import_id``, ``rows``, ``labels`` and ``searchable``. A missing file prints ``Error: memory: PATH is not a file. `ljos memory export --out PATH` writes one on the machine that has the lessons`` and exits 1. When the path is a directory the line is ``Error: memory: PATH is a directory. Pass the file `ljos memory export --out` wrote`` and exits 1.
``memory integrate [ID|--all] [--dry-run] [--json]``
   Stores: pack, tracker. Map one import, or every import with ``--all``, onto lessons already stored. The same text keeps one canonical id. A second run of an import that is already integrated prints ``nothing new: N rows already integrated`` and reads no further. ``--dry-run`` prints ``dry-run`` and the mappings, and writes nothing. ``--json`` prints the mappings and adds ``imported``, ``unchanged``, ``skipped`` and ``reason``. With no id and no ``--all`` the line is ``Error: memory: pass an import id or --all. `ljos memory imports` lists the ids. `ljos memory integrate --all` maps every import`` and exits 1. Both together print ``Error: memory: pass an import id or --all, not both. `ljos memory imports` lists the ids`` and exits 1. An unknown id prints ``Error: memory: no import starts with PREFIX. `ljos memory imports` lists the ids`` and exits 1. When the prefix matches two or more ids the line is ``Error: memory: PREFIX starts N imports. Pass a longer id from `ljos memory imports``` and exits 1. An import with no rows uses ``Error: memory: import ID has no rows. `ljos memory imports` lists the slots`` and exits 1. ``--all`` with no imports prints ``no imports. `ljos memory import FILE` stores a file `ljos memory export --out` writes``. ``ljos mesh pull`` integrates the same way and writes the seat's seed at ``<tracker>/.ljos/memory/mesh/<seat>/seed.jsonl``.
``memory imports [--json]``
   Stores: tracker. List the slots. An empty list prints ``no imports``. ``--json`` prints those rows.
``sync [--key] [--import] [--export]``
   Stores: tracker, pack. Share the seat's memory across machines through the tracker repository: pull and take other machines' sealed logs, then write and push this machine's; ``--key`` prints this machine's age public key (made on first use) and stops; ``--import`` only pulls; ``--export`` only writes and pushes. ``--adopt HOST`` folds a gone host's atoms into this host's log; ``--force`` adopts a host whose log was committed within 14 days; ``--dry-run`` prints that and writes nothing. ``--adopt-legacy HOST`` reads that host's logs from before the scope header. ``--compact`` folds this machine's atom segments into one snapshot.
``merge-atoms CURRENT PATH``
   Stores: tracker. Re-export this machine's atoms over a conflicted tracker log. git calls this as the ``ljos-atoms`` merge driver, ``%A`` then ``%P``.
``handover --out DIR [--project P]... [--issue I]... [--to user@host:path] [--since-size N] [--scope NAME]``
   Stores: all. Pack, seal, sign; ``--to`` copies the bag to another seat over ssh; ``--since-size`` puts the bridge from the size the receiver kept into the bag; ``--scope`` carries only the atoms tagged ``scope:NAME``, sealed with age to that scope's recipients, and no atom in plaintext.
``receive DIR [--since KEPT] [--keep FILE] [--import]``
   Stores: all. Check, and import the atoms with origin ``peer``; ``--since`` is the head ``--keep`` wrote at the last receive from this sender, and the bag's bridge must start there; a sealed scope file opens with the identities that scope allows, and stays closed without them.
``doctor [--all] [--fix]``
   Stores: all. Check that ljos is ready, and print the rows that need action. Exit 1 when a required program, the memory store or its token does not answer, the tool server a runner starts or the cloud signing key does not, the note queue is damaged, or the tracker directory is not writable. A tracker that has not been created yet does not fail this command. ``--all`` (alias ``--verbose``) prints every row, the ones that answer included. ``--fix`` refreshes runner files this ljos wrote, and repairs a missing ljos home or config file, a stale lock or pid file, a pack writer that is not running, and a runner session with no seat recorded. It prints one line for each repair and one line for each problem it left. A probe that does not answer is a row that says it timed out, and it does not hold the report. A ``panes`` row names the tool that opens persona panes; with ``jev.toml`` present, a ``jev`` row carries the month's calls, cached answers and spend. A ``git identity`` row is a warning when git has no author name or email, because the tracker commit is refused; it names the two ``git config`` commands that set ``user.name`` and ``user.email``. Example: ``ljos doctor``.
``seat [add NAME | grant SEAT close=own --quote TEXT --at TIME --via SEAT]``
   Stores: tracker. Who is sitting. When the source is the most recent write on this machine, the next line names ``export LJOS_SEAT=NAME``. ``add`` writes NAME into the tracker's ``vissue.toml`` seat list; ``grant`` records a quoted yes that SEAT may close tickets it filed or holds. ``--quote`` is the owner's yes, quoted as they said it. ``--ids-file`` names those tickets, one id a line. Example: ``ljos seat``.
``protocol [--full]``
   Stores: none. Print how work on one ticket is meant to run. ``--full`` prints every store and refusal.
``onboard [--harness NAME] [--skills DIR] [--dry-run] [--example]``
   Stores: runner. With no ``--harness``, ljos onboards every runner it finds, at a terminal or in a pipe. A pipe prints the one ``mcpServers`` entry on stdout and the setup lines on stderr. The tracker, the memory writer and the deed store are set up either way. The Model Context Protocol (MCP) is the tool server the runner starts. Onboard in json form prints that entry at a terminal too. With a runner named in ``~/.config/ljos/harnesses.toml``, register ``ljos-mcp`` with it, install the protocol as its skill, and write the personas into its ``agents`` directory; a shell runner (``shell = true``, ``example-host``, or ``shell``) writes the skill and an env file and registers no server and no hook; changed runner files and their previous bytes are recorded for ``offboard``. A step that does not land exits 1. Example: ``ljos onboard``.
``offboard [--runner NAME] [--dry-run]``
   Stores: runner. Restore runner files changed by ``onboard`` and remove files created only for ljos; ``--runner`` (alias ``--harness``, the same name ``ljos onboard`` uses) limits removal to one runner; ``--dry-run`` reports without writing; later edits are preserved; an older installation without a manifest is removed by its ljos markers. A file edited after onboarding, when the ljos entry cannot be taken out, is left in place. The line names the file and says to delete the ljos entry and run ``ljos offboard`` again. A step that does not land exits 1.
``uninstall [--dry-run] [--keep-programs] [--all] [--purge] [--no-stub] [--bin-dir DIR]``
   Stores: runner, programs. Offboard every runner, put the global git hooks back, stop the hook server, remove the ljos programs and leave a stub named ``ljos`` that answers every hook with an allow, so a session that cached the hook path is not blocked; ``--all`` removes the other seat programs, ``--purge`` the state, data and configuration directories; the release ships the same as ``uninstall.sh`` for a machine the binary was deleted from. A file that still runs ljos after offboard is named. Edit it and delete the ljos entry. The programs are still removed.

``completions bash|zsh|fish|powershell|elvish``
   Stores: none. Print a shell completion script. The script completes subcommands and flags. Seat names and open ticket ids are included when the tracker is small enough to read while the shell waits. A larger tracker completes commands and flags only. Reading the script does not start ljos. ``install.sh`` prints the line for each shell and does not write a shell startup file. Bash reads ``~/.bashrc`` and the line is ``source <(ljos completions bash)``. Zsh reads ``~/.zshrc`` and the line is ``source <(ljos completions zsh)``. Fish reads ``~/.config/fish/config.fish``, or the file ``~/.config/fish/completions/ljos.fish``, and the line is ``ljos completions fish | source``. PowerShell reads the profile ``$PROFILE`` and the line is ``ljos completions powershell | Out-String | Invoke-Expression``. Elvish reads ``~/.config/elvish/rc.elv`` and the line is ``eval (ljos completions elvish)``. Example: ``ljos completions bash``.
``help [--all] [COMMAND...]``
   Stores: none. Print this list, or the help of one command. ``--all`` lists every command, including those left off the short list. ``ljos`` with no arguments prints what ljos is, the seat and where it came from, then ``install``, ``doctor``, ``remember`` and ``search``, and names this page. The install line is left off when this binary is the installed program. A read with no seat named says it uses every seat, because no seat was named.
``options ISSUE A,B``
   Stores: tracker. Append ``Options: A, B`` to an issue. Two or more, separated by commas. Fewer than two is refused.
``block ISSUE [OTHER...] [--on ID|--off ID]``
   Stores: tracker. Record that one ticket waits on another. A following id is the same as ``--on``. Further ids are further tickets it waits on. ``--on`` adds that edge. ``--off`` drops it. A following id beside ``--on`` or ``--off`` is refused.
``parent ISSUE PARENT``
   Stores: tracker. Move ``ISSUE`` under ``PARENT`` by editing only that ticket's parent link. The write refuses an unknown parent, refuses a cycle, records a dated note on ``ISSUE``, and commits the tracker.
``export --out FILE [--machine NAME] [--json]``
   Stores: pack, tracker. The same command as ``memory export``.
``proposals [--seat SEAT] [--status STATUS] [--settle]``
   Stores: pack. List lesson proposals waiting on a review. ``--seat`` keeps one seat. ``--status`` is open, proposed, accepted, refused, already-in-pack, in-pack, or already in pack. Unknown status values exit 2. ``--settle`` records already-in-pack on open rows the pack already holds, and deletes nothing.
``proposals amend ID --text TEXT``
   Stores: pack. Record a revised text and run the readability checks, without accepting. ``--text -`` reads stdin.
``merge-org BASE OURS THEIRS``
   Stores: tracker. Field-merge one issue file. git calls this as the ``ljos-org`` merge driver, ``%O`` ``%A`` ``%B``. The merged text replaces ours.
``judge-score``
   Stores: pack. Score the judge log against outcomes: accuracy and calibration per decision.
``person-key set|clear|status``
   Stores: machine. Set, clear, or show the passphrase that unlocks this person's key. ``set`` and ``clear`` read the terminal. They are refused when a runner variable is set.
``cloud up [--force] [--repoint]``
   Stores: release, tracker. Install the release binaries, write the tool command a runner starts. That command is the Model Context Protocol (MCP) entry, and clone ``LJOS_TRACKER_REMOTE``. ``--force`` downloads again. ``--repoint`` replaces the repository an earlier run recorded. Example: ``ljos cloud up``.
``mesh up [--key-file PATH] [--login-server URL] [--hostname NAME]``
   Stores: machine. Join the private network and print this machine and the others on it. Headscale is the login server. A failure is one line on stderr and a non-zero status. Example: ``ljos mesh up --key-file ~/.config/ljos/mesh.key``.
``mesh down``
   Stores: machine. Log out and stop the tailscaled ``mesh up`` started. When this machine is not on the mesh, print ``mesh down: not on the mesh`` and exit 0. A failure is one line on stderr and a non-zero status.
``mesh status``
   Stores: machine. Print this node and the peers ``tailscale status`` reports, with their tags.
``mesh acl [SWARM...]``
   Stores: machine. Print who on the private network may connect to whom. The rules are the access policy ``mesh.toml`` implies. ``up`` and ``down`` do not read that file.
``mesh pull --seat SEAT --from DIR``
   Stores: pack, tracker. Read another machine's memory files into a seat and keep a copy of the rows brought in. An unchanged export is not read again. A second pull of unchanged exports does not call the pack or take the writer lock, and a busy pack is not waited out. The second line is ``exports skipped S, imported I``. A lane is one machine's memory file. A missing directory prints ``Error: mesh: PATH does not exist. Create it, or pass a directory of <lane>.jsonl files`` and exits 1. A path that is a file prints ``Error: mesh: PATH is not a directory. Pass a directory of <lane>.jsonl files`` and exits 1. A memory store that stops the pull prints ``Error: mesh: the pull stopped: CAUSE, and lessons already stored stay in the seed with rows still missing, so run `ljos mesh pull --seat SEAT --from DIR`.`` and exits 1.
``mesh push --seat SEAT --to DIR [--lane NAME]``
   Stores: pack. Write a seat's memories into a handoff directory as one lane file.
``mcp [ARGS...]``
   Stores: none. Start the tool server a runner connects to. This runs the ``ljos-mcp`` installed beside this ``ljos`` and passes the arguments through. MCP is that tool server.
``git-hook NAME``
   Stores: git. Run the ljos part of one git hook. The block ``ljos git-hooks install`` appends calls this.
``slot ISSUE SEAT --model MODEL --run RUN --key HEX``
   Stores: tracker. Sign the record that lets a cloud run vote on a ticket, before that run's first ballot.

The manifest and backups are under ``${XDG_STATE_HOME:-~/.local/state}/ljos/onboard``.
They are removed after every recorded runner is offboarded.

``ljos file "TITLE" --options A,B`` writes ``Options: A, B`` as the first body line.
``ljos options ISSUE A,B`` appends that line and refuses fewer than two.
Withdrawing a ballot writes ``ballot-withdrawn: VOTER HARNESS NONCE``.
The tally leaves that ballot out, and so does the push check.
A later cast from that session says the ballot was withdrawn.

``send SEAT TEXT`` writes to one seat. A seat the pack has never heard
from is refused, with the seats it knows, since mail waits under the name
it was sent to; ``--new-seat`` writes to one that has not started yet.
``send --group NAME TEXT`` writes to the other members of that group. ``--interrupt`` leads the next prompt.
``--issue ID`` threads the message on that vissue issue, and the atom takes
the issue's scope so ``ljos sync`` carries it in that repository's sealed
log. ``inbox`` lists unread mail and writes a receipt for each unread message it shows. A pack that does
not answer is an error. A writer that refuses kind ``message`` is an error
too: the command exits non-zero, names packset 0.13.0, and does not print
an empty inbox. When the writer reports a version, the error names that
version. ``read ID`` writes one receipt. ``reply ID TEXT``
answers the sender and keeps the issue. ``group NAME --add SEAT`` records
a member. ``ljos hook --prompt`` shows at most eight unread messages,
interrupts first, and writes one receipt for each message it shows.
The rest stay unread, and the note says how many remain.
The receipt is an atom whose text is four lines: ``read ID``, ``by SEAT``,
``at STAMP`` and ``msg ID``. The sender's inbox prints that receipt once,
as the message id, the reader and the stamp. A second prompt writes
nothing. A shell writes the receipt before the hook exits. The editor harness holds
the note: the prompt prints nothing and writes no receipt, and the
receipt is written when the note is delivered, on ``postToolUse`` as
``additional_context``, or on ``Stop`` when the turn ran no tool, as
``followup_message``. When the pack does not answer, the hook prints
``mail could not be checked`` and still exits 0. A shell seat polls ``ljos inbox``.

``send`` and ``reply`` start the recipient's wake, when it has one, after the message is written.
``ljos wake --command CMD`` runs ``CMD`` under ``sh -c`` with ``LJOS_SEAT`` set to the seat, so ``ljos inbox`` in it reads that seat's mail.
With a prefix wake, ``LJOS_SEAT`` is still the run seat that matched the prefix.
``LJOS_WAKE_FROM``, ``LJOS_WAKE_MSG``, ``LJOS_WAKE_ISSUE`` and ``LJOS_WAKE_INTERRUPT`` name the message.
``ljos wake --webhook URL --key-file FILE`` posts the envelope (seat, sender, id, issue, interrupt, and ``via`` when a prefix matched) to an https URL with the key in that file as a bearer token, and never the body.
A key is a prefix only when it ends in one trailing ``-`` after a non-empty stem; names such as ``-``, ``--`` and ``a--`` are exact names.
The sender waits on neither, and a wake that fails or runs past its timeout is a line in ``~/.local/state/ljos/mail/wake.log``.
On ``Stop``, unread interrupt mail holds the turn once and is receipted, so a running session reads it before it goes idle.
A seat the pack has never heard from is refused with the known seat it most likely meant.

``ljos deed --remove ACCESSION --ticket ID`` drops that citation. The removal is appended to the ticket. The heading is not rewritten.

``remember`` and ``prefer`` write origin ``user-declared`` for the person and
``agent-derived`` for an agent, at once, from any caller. ``finish --lesson``
stores the lesson at once at L0, packset's ``derived`` level, with origin
``agent-derived``, and as ``user-declared`` when the person types it in a
terminal of their own and no runner marks the environment.
``findings --remember`` stores each lesson at L0 with origin ``tool-echo``,
because a campaign file is tool output. The prompt hook stores a
correction in the prompt's words, marked ``hook``: origin ``user-declared``
when the person's own prompt said it, else ``agent-derived`` at L0. Search
finds each at once. ``ljos accept ID`` raises a memory from L0 to
``explicit`` and records a ``keep`` review; the origin stays. A proposal an
older ljos left open in ``proposals.jsonl`` is stored once by the next
``SessionStart`` or prompt hook, with the origin and level it was filed
under. ``ljos proposals`` lists the open rows the pack does not already
hold, then any refused row. A refused row quotes the sentence the pack
measured, after the count and the limit, trimmed to about 80 characters,
as in ``sentence has 33 words; limit 29: "git push needs to run inside..."``.
The last line counts the open rows, for example :literal:`70 open: 52 new, 18 already in pack (\`--status in-pack\` to list them)`.
An empty queue does not read the pack. A busy memory service is not waited out: the list is the local snapshot, and the line is ``memory service busy; listed from the local snapshot``.
``--status in-pack`` lists the ones whose text and kind already match a pack atom.
``ljos proposals --settle`` writes ``already-in-pack`` and the time on those rows, prints one line per id and a total, and deletes nothing.
A second run writes nothing.
``ljos proposals amend ID --text TEXT`` records a revised text, keeps the original, and runs the same readability check without accepting.
``--text -`` reads the text from stdin.
A pass prints ``ok ID: passes (ready for ljos accept)``.
A miss prints ``refused ID: sentence has 33 words; 33 vs limit 29: "..."``.
``ljos accept`` stores the latest amended text and marks the atom ``provenance:amended``.
Accepting a refused proposal again, with no amendment since, prints ``refused ID: unchanged since last refusal at TIME; amend it with ljos proposals amend`` and does not run the check again.
``ljos proposals --status refused`` quotes the sentence and gives that amend hint. An atom taken by ``receive --import`` or by ``sync`` has origin
``peer``, whatever origin the sender wrote.

The claim graph records the held ticket too: a tracker id maps to one node
(a 128-bit FNV-1a hash of the id) and a name to one actor; a 32-hex id passes through.

On Linux the pack client reads who owns the socket on its port from
``/proc/net/tcp``. A ``PACKSET_PORT`` that another user listens on is
refused. With nothing set and 8761 another user's, the seat uses port
18761 plus its user id modulo 10000 and exports it as ``PACKSET_PORT``, so
``packset ensure`` starts its writer there. ``PACKSET_URL`` is used as given.
Without ``XDG_RUNTIME_DIR``, the hold records and the seat records go in
a directory named for ``HOME``, such as ``ljos-UID/home-1a2b3c4d5e6f``, under the
temporary directory, mode 0700, so two homes of one user do not share a hold.
They go in ``~/.cache/ljos/run`` when ``ljos-UID`` is another user's or a symlink.

This flag list is copied from command help.
A test fails when this list and the binary differ.

..
   # proseguard:off

.. BEGIN ljos-help-flags

accept --at --quote --via
agents --dry-run --harness
ask
block --off --on
brief
bump-plan --dry-run --generation --parent --project
calibrate --project --rounds
cards --cards --dir
claim --assignee --next --role --slack --takeover
claims --agent --by --days --json --mine --project --stale
cloud
cloud up --force --repoint
complete --assignee --gen --status
completions
conflicts --limit
consensus --compare --deadline --json --no-file --quorum
consolidate --apply
current
deed --add --porcelain --remove --ticket
digest --json --mark --since
doctor --all --fix --verbose
due --all --json --judge
evidence
export --json --machine --out
file --body --deadline --json --options --parent --porcelain --priority --project --quorum --tags --top --type --verbose
findings --all --issue --remember
finish --assignee --beta --close --gen --json --lesson --new-option --outcome --quiet --status --verbose
forget --json --why
git-hook
git-hooks
graded --lapsed
group --add --remove
habit --every --source --unit
handover --issue --out --project --scope --since-size --to
help --all
hook --brief-once --drain --event --fail-on-deny --git-commit --git-hook --limit --prompt --runner --text
hubs --json --limit
hud --foreground --hide --install-desktop --show --toggle
inbox --all --sync
island --as --fire --json
judge-score
learn --beta --new-option --outcome --rule --share
list --all --json --limit --mine --project
log --seat
mcp
memories
memories list --all --json --seat --since --user
memories prune --apply --days --json --stale
memories review --all --drop --edit --json --keep --merge --seat --since --text --user
memories tui --all --seat --since --user
memory
memory export --json --machine --out
memory import --as-machine --json --source-machine --source-seat
memory imports --json
memory integrate --all --dry-run --json
merge-atoms
merge-org
mesh
mesh acl
mesh down
mesh pull --from --seat
mesh push --lane --seat --to
mesh status
mesh up --hostname --key-file --login-server
note --json --quiet --verbose
offboard --dry-run --harness --runner
onboard --dry-run --example --harness --skills
open-panel --cwd --log --prompt-file --session
options
panel --cloud --designers --dry-run --harness --jev --out
parent
person-key
persona --about --anchor --check --replace --retire --runner --view
personas
playbook
playbooks
policy --fail-on-deny
predict --as --expect
prefer --as --ask --deny --deny-mention --deny-name --deny-reply-name --deny-reply-to --forget --keep --promote --review --sync --user --why
proposals --seat --settle --status
proposals amend --text
protocol --full
read
ready --all --json --limit --mine --project
reap --apply --idle
recall --json --no-graph
receive --import --keep --since
record
record check --tracker
release --assignee --days --dry-run --older-than --stale --verbose --yes
remember --as --json --project --standing --transient --valid-from --valid-to --verbose
reopen
reply --interrupt
retire-notes
rule --json --user --verdict --why
rules --json
search --as-of --attempts --explain --import-id --json --limit --min-score --porcelain --project --rerank --source-machine --source-seat --valid-at
seat
seat add
seat grant --at --ids-file --quote --via
send --group --interrupt --issue --new-seat
show --all --json --verbose
sitting --anyway --assignee --cards --dir --json --plan --playbook --project --top --verbose
slot --key --model --run
statusline
supervise --at --cwd --exit --failures --harness --max-restarts --restart --script --window
sync --adopt --adopt-legacy --compact --dry-run --export --force --import --key
timeline --json --limit
trust --about --why
uninstall --all --bin-dir --dry-run --keep-programs --no-stub --purge
upgrade --dir --only
usage --since
vote --against --as --child --confidence --expect --for --jev --used --withdraw
wake --clear --command --fire --from --interrupt --issue --key-env --key-file --msg --timeout --webhook
withdraw --as --note
workflow --round --start
.. END ljos-help-flags

..
   # proseguard:on

