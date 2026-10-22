:description: The ljos command line, the hooks that book commits, the user rules, and the environment the seat reads.
:cover: https://leidarljos.github.io/ljos/_static/og.png

Command line
============

+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| Verb                                                                                                                            | Habitat                   | Does                                                                                                                                                                                                                                           |
+=================================================================================================================================+===========================+================================================================================================================================================================================================================================================+
| ``sitting ISSUE [--assignee NAME] [--cards DIR] [--playbook NAME] [--anyway]``                                                  | all                       | open a sitting: doctor, sync when the repository names a scope, cards, due, island, blockers, playbook, workflow when the issue carries one, panel when the issue is a decision, recall, timeline, reclaim, claim; reclaim hands back claims   |
|                                                                                                                                 |                           | quiet longer than ``LJOS_LEASE_SECS`` (900; 0 is off) whose conversation is gone, and keeps one whose runner still runs; stops at the first store down; the name defaults to this conversation's holder ( ``ljos seat`` ); ``--playbook``      |
|                                                                                                                                 |                           | copies a recipe before recall; absent, title-match else ``sit`` ; the name is a tracker ``playbook:`` note until finish or release                                                                                                             |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``finish ISSUE [--status S] [--lesson TEXT] [--outcome OPTION] [--beta B] [--gen G] [--assignee NAME] [--close]``               | all                       | close a sitting: complete the session node, file ``--lesson`` as a proposal at L0 (``derived``) with origin ``agent-derived`` or write it at once as the person's when no agent runner is above the process and a terminal is on its input,    |
|                                                                                                                                 |                           | fire the island, then learn; ``ljos accept ID`` promotes that proposal to ``explicit`` and writes the lesson; ``--gen`` names the claim's generation (absent, the live one; a stale one is refused); ``--assignee`` names the holder (absent,  |
|                                                                                                                                 |                           | this conversation's holder); ``--close`` closes the ticket only when the work is accepted, else the ticket stays open and nobody holds it                                                                                                      |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``calibrate -p PROJECT [--rounds N]``                                                                                           | consensus, pack           | trust rows from the project's voting history (Dawid-Skene accuracy)                                                                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``remember TEXT [--as PERSONA] [--transient|--standing]``                                                                       | pack                      | one lesson, stored as written, origin ``user-declared``; as a persona, it carries that persona's entity and opens its next brief; typed by the person it is standing at once, and the hook shows it from the next prompt it bears on; run      |
|                                                                                                                                 |                           | under an agent runner it is an episode, kept and not a refresher until a review promotes it; ``--transient`` or ``--standing`` picks one either way                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``memories list|review|tui|prune``                                                                                              | pack                      | review the memories agents wrote on their own: ``list`` newest first with provenance, unreviewed agents' memories by default; ``review`` keeps, drops, edits or merges them; ``tui`` browses them; ``prune`` names duplicates and              |
|                                                                                                                                 |                           | never-recalled memories and writes only with ``--apply``                                                                                                                                                                                       |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``prefer TEXT [--deny PATTERN] [--ask PATTERN] [--user --why REASON]``                                                          | pack, user config         | one standing preference, origin ``user-declared``; each ``--deny`` or ``--ask`` also writes that rule with the preference as its reason, so the hook enforces it. ``--user`` writes a user rule instead, shared by every seat on this machine, |
|                                                                                                                                 |                           | never reviewed, and not also stored as a seat preference; ``--why`` is required with it. See User rules                                                                                                                                        |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``accept ID``                                                                                                                   | pack                      | promote one open proposal from L0 (``derived``) to ``explicit`` and write it into the pack; the origin stays; under an agent runner, only a proposal another conversation filed; a proposal the same text already wrote with ``remember`` or   |
|                                                                                                                                 |                           | ``prefer`` is not written again                                                                                                                                                                                                                |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``forget ID [--why DEED]``                                                                                                      | pack                      | tombstone a claim, naming the deed that withdrew it                                                                                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``search QUERY [-n N] [--rerank] [--as-of TIME] [--explain]``                                                                   | pack                      | ranked claims with how many scorers named each and its age; ``--rerank`` scores the top hits with the writer's cross-encoder; ``--as-of`` asks the pack as it stood then; ``--explain`` opens with what the score and the fraction mean        |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``timeline ISSUE [-n N]``                                                                                                       | tracker, deed store, pack | the logbook, the cited deeds and the activated memories as one dated list, oldest first, each line with its age and the gap since the one before                                                                                               |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``consolidate [--apply]``                                                                                                       | pack                      | the pairs where a later claim rewrites an earlier one; ``--apply`` closes the earlier ones' windows                                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``conflicts [-n N]``                                                                                                            | pack                      | candidate contradictions by geometry: the lowest passes between single memories, read by the optional ``landscape`` habitat. The habitat is not part of this repository. When it is not on ``PATH`` the command says so and writes nothing     |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``hubs [-n N]``                                                                                                                 | pack                      | the claims this seat's memory turns on, highest first: a weighted PageRank over the links                                                                                                                                                      |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``island CUE [--fire]``                                                                                                         | pack                      | the memories a task activates: search hits spread along the links; ``--fire`` wires the strongest eight together                                                                                                                               |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``due``                                                                                                                         | pack                      | sweeps the pack first (reviews left due past twice their interval lapse; never-recalled claims missed three times are forgotten), then the claims whose review is due, unreviewed ones first, one line on the clock, and one on what the sweep |
|                                                                                                                                 |                           | moved                                                                                                                                                                                                                                          |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``hud [--foreground] [--toggle|--show|--hide] [--install-desktop]``                                                             | none                      | exec sibling ``ljos-hud``; missing is 127, hide-miss is 0; the musl CLI never links iced                                                                                                                                                       |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``habit [NAME [VALUE]] [--unit U] [--every 7d] [--source S]``                                                                   | pack                      | with a value, take a reading: the earlier reading closes and is kept as what it was, the next is due one cadence on; without, list each habit's latest reading with the change since the one before                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``graded ID [--lapsed]``                                                                                                        | pack                      | one review graded                                                                                                                                                                                                                              |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``trust FROM TO WEIGHT [--why DEED]... [--about DOMAIN]...``                                                                    | pack                      | one trust row; scoped when ``--about`` is given                                                                                                                                                                                                |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``persona NAME --anchor A --view TEXT [--about DOMAIN]... [--runner RUNNER] [--replace] [--check]``                             | pack                      | a voter with a view; ``A`` in [0, 1] is how far it moves off its ballot; writes one unscoped inbound trust row; ``--runner`` names a ``[[harness]]`` that reasons as the persona in a session it keeps; a name another seat or conversation    |
|                                                                                                                                 |                           | defined is refused unless ``--replace``; ``--check`` says what the define would do and which personas share its view or domains, and writes nothing                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``persona NAME --retire [--replace]``                                                                                           | pack                      | withdraws the persona, so it casts no more ballots; only the conversation that defined it may, unless ``--replace``                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``personas``                                                                                                                    | pack                      | the roster: name, anchor, domains, view, one per line                                                                                                                                                                                          |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``agents [--harness NAME] [--dry-run]``                                                                                         | pack, runner              | every persona as an agent definition, ``ljos-NAME.md``, in each runner's ``agents`` directory (or one runner's): a subagent the runner spawns by name that briefs, casts one ballot before reading the others, notes why and stops;            |
|                                                                                                                                 |                           | definitions whose persona left the pack are removed                                                                                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``statusline``                                                                                                                  | pack                      | one line for a runner's status bar (Grok's ``[ui.status_line]``, Claude Code's ``statusLine``): the seat, the issue this conversation has claimed, the claims due; reads the runner's status JSON on stdin; each session's line is cached for  |
|                                                                                                                                 |                           | fifteen seconds                                                                                                                                                                                                                                |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``ask NAME TEXT``                                                                                                               | persona session           | hands the persona a question or a task in its own pane, opening it (and resuming its session) when it is closed                                                                                                                                |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``supervise NAME --harness H --exit clean|crash --at SECONDS``                                                                  | runner                    | classify one harness exit and print ``restart``, ``stop`` or ``give-up``, and on a restart the herdr line that would start the child. It prints the line and does not run it. ``--restart`` is ``permanent`` (the default), ``transient`` or   |
|                                                                                                                                 |                           | ``temporary``; three restarts inside sixty seconds give up                                                                                                                                                                                     |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``upgrade [VERSION] [--dir DIR]``                                                                                               | release                   | installs a published release beside the running ``ljos``: the archive the release workflow built for the tag, checked against the sha256 published beside it, each binary an executable that names the version; the old ones kept as           |
|                                                                                                                                 |                           | ``NAME.bak-pre-VERSION``. The only way an agent moves the seat's binaries, which the guard keeps it from writing                                                                                                                               |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``playbook ISSUE NAME``                                                                                                         | pack                      | bind a recipe to the issue and copy its full body; the name is a tracker ``playbook:`` note until finish or release                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``playbooks``                                                                                                                   | pack                      | the closed set sit, arena, land, company-panel, overnight, feature; pack latest per name, the built-in seed when missing                                                                                                                       |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``workflow ISSUE [--start NAME] [--round]``                                                                                     | tracker                   | a parent's workflow: stage, round and next step; ``--start feature`` notes ``workflow: feature`` and files the design child, ``--round`` files the next panel round once every child is closed                                                 |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``brief NAME ISSUE``                                                                                                            | pack, tracker             | what a subagent playing the persona starts from: view, the bound playbook's full recipe, five named principles, the arena rubric, what the seat knows on its domains, the working set                                                          |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``panel ISSUE [--out DIR] [--jev]``                                                                                             | pack, tracker             | one brief per persona whose domains the issue speaks to as ``DIR/<name>.md``, then the settle line; refused until a playbook is bound. ``--jev`` asks Jev for every ballot first and casts them only when every seat is sure and all agree;    |
|                                                                                                                                 |                           | otherwise it writes the briefs                                                                                                                                                                                                                 |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``open-panel --prompt-file FILE --log FILE [--cwd DIR] [--session ID]``                                                         | tracker, runner           | open a panel for a decision prompt: file or reuse the issue, sit it, and start one headless member per brief. The prompt is already in the file, so the hook can return. ``--session`` keeps a second opener from starting a second panel      |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``learn ID --outcome OPTION [--rule record|hedge] [--beta B] [--share S]``                                                      | tracker, pack             | reweigh voters by what turned out right: by default each voter's record of hits and misses as log-odds weights, its accuracy shrunk toward the panel's pooled accuracy; ``hedge`` shrinks refuted voters by ``B`` with ``S`` recovery; either  |
|                                                                                                                                 |                           | way the outcome is kept                                                                                                                                                                                                                        |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``evidence ACCESSION`` / ``current ACCESSION``                                                                                  | deed store                | intact; still the tip                                                                                                                                                                                                                          |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``record check DIR`` / ``record check --tracker ROOT``                                                                          | none / tracker            | ``DIR`` holds schema-v1 question and ballot entries; ``--tracker`` holds every ballot under ``ROOT``, a persona's and a judge's included, to the push gate's check: the voter's newest ``ballot-sig:`` note names the choice, verifies, and is |
|                                                                                                                                 |                           | under the key ``.ljos-seat-keys`` pins for that voter and no other seat. One ``FILE:LINE: ISSUE: REASON`` line per ballot that does not stand, and exit 1. An unsigned ballot committed before signing began is legacy. It only reads, so CI   |
|                                                                                                                                 |                           | can run it                                                                                                                                                                                                                                     |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``file TITLE [--parent ID|--top] [-p PROJECT] [-t TYPE] [--tags T] [--priority P] [--body TEXT]``                               | tracker                   | file work found while sitting: a child of the held issue unless ``--parent`` names another or ``--top`` none, in the parent's project; a projected board takes it into its inbox under the printed id; the tracker file is committed           |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``note ID TEXT...``                                                                                                             | tracker                   | a dated progress note on the issue, committed; on an issue another seat holds it says so: the note is an assist, and ``ljos send`` reaches the holder; the pack keeps it as a ``summary`` tagged ``issue:ID``, which search and recall find    |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``log --seat SEAT``                                                                                                             | tracker                   | tracker commits by one seat. The subject is ``chore(issues): ID VERB by SEAT``                                                                                                                                                                 |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``usage --since YYYY-MM-DD``                                                                                                    | tracker                   | how many tracker commits each seat made from that day on                                                                                                                                                                                       |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``send SEAT TEXT... [--group NAME] [--interrupt] [--issue ID]``                                                                 | pack                      | write to one seat, or with ``--group`` to the other members of that group; ``--interrupt`` leads the next prompt; ``--issue`` threads the message on that vissue issue and the atom takes the issue's scope, so ``ljos sync`` carries it in    |
|                                                                                                                                 |                           | that repository's sealed log                                                                                                                                                                                                                   |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``inbox [--all]``                                                                                                               | pack                      | mail this seat has not read, then receipts on what it sent that it has not been shown; listing writes no receipt; ``ljos hook --prompt`` writes a receipt for each message it shows; a pack that does not answer is an error, and so is a      |
|                                                                                                                                 |                           | writer that refuses kind ``message``; ``--all`` includes mail already read                                                                                                                                                                     |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``read ID``                                                                                                                     | pack                      | write a read receipt for one message; a second read writes nothing                                                                                                                                                                             |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``reply ID TEXT... [--interrupt]``                                                                                              | pack                      | answer the sender, keep the issue, and carry ``reply:ID``                                                                                                                                                                                      |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``group NAME [--add SEAT]... [--remove SEAT]...``                                                                               | pack                      | list a group, or add and drop members; membership is atoms and travels in the sealed log                                                                                                                                                       |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``wake [SEAT] [--command CMD [--timeout S]|--webhook URL [--key-file F] [--key-env VAR]|--clear|--fire]``                       | machine                   | how mail wakes a seat that is idle: a shell line this machine runs with ``LJOS_SEAT`` set to the seat, or a URL it posts the envelope to (never the body); ``send`` and ``reply`` start it in its own session and do not wait on it; the guard |
|                                                                                                                                 |                           | judges a command when it is set and on every run, and a ``deny`` or an ``ask`` keeps it from running; ``--fire`` runs it now and prints what happened; alone, show the wake; kept in ``~/.config/ljos/wake.toml``                              |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``deed ID [--add ACCESSION]...`` / ``--remove ACCESSION --ticket ID``                                                           | tracker                   | cite with ``--add``, or list; ``--remove`` drops one citation, appended to the ledger, and the heading is not rewritten                                                                                                                        |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``claims [ARGS]``                                                                                                               | tracker                   | who has claimed what: ``vissue claims`` with its flags (``--by NAME``, ``--json``)                                                                                                                                                             |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``recall ID``                                                                                                                   | tracker                   | the working set                                                                                                                                                                                                                                |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``vote ID [--for OPTION] [--confidence P] [--used ACCESSIONS|none] [--expect OPTION|JSON] [--as PERSONA] [--jev] [--withdraw]`` | tracker                   | cast, as the seat or as a persona, or read the tally; ``--confidence`` is a probability in (0, 1]; ``--used`` lists the deed accessions the ballot used, comma-separated, or ``none``, and is required with ``--for``; ``--expect`` records    |
|                                                                                                                                 |                           | the forecast of the others on the same command; ``--withdraw`` takes back the ballot and its forecast, and the logbook keeps what it was. ``--as NAME --jev`` asks Jev for the persona's ballot: cast with the option's probability and a      |
|                                                                                                                                 |                           | forecast at confidence 0.8 or above, noted and left for a subagent under it                                                                                                                                                                    |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``predict ID --expect OPTION|JSON [--as PERSONA]``                                                                              | pack                      | forecast the others' shares; two or more and ``consensus`` names the answer more popular than forecast (Prelec, doi:10.1038/nature21054)                                                                                                       |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``rule PATTERN [--verdict deny|ask] --why TEXT [--user]``                                                                       | pack, user config         | argv law the hook and ``policy`` enforce: a glob, or a regular expression (``re:`` or a ``\b``, ``\s`` or ``(a|b)`` in it) anchored at the start, tried on the whole line and on each command it runs; What a rule reads lists which those     |
|                                                                                                                                 |                           | are. ``--why`` is the reason a stopped reader sees. ``--user`` writes the law into the user config instead of the seat pack, shared by every seat and never reviewed                                                                           |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``rules [--json]``                                                                                                              | pack                      | the rules the hook and ``policy`` judge by: verdict, pattern and reason, deny first                                                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``consensus ID [--compare]``                                                                                                    | consensus, tracker        | first what a push cite on the issue decides, from seat ballots alone; then the settle under the pack's rows that apply to the issue, with every persona's anchor, as advice; an issue tagged ``broad`` runs bounded confidence; once five      |
|                                                                                                                                 |                           | issues have an outcome, voters shown to err together are discounted; ``--compare`` adds the tracker's own settle, and so does a missing model crate; an issue with no ballots is refused before a settle runs; ``--json`` prints one JSON      |
|                                                                                                                                 |                           | document, the push cite at the top level and each reading under its own key                                                                                                                                                                    |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``claim [ID] [--next] [--role ROLE] [--slack N] [--assignee NAME]``                                                             | claim graph               | claim a session node. With ``--next``, or with no id, it claims the first ready node in claimdag's order for the name, the way ``claim ID`` claims one; ``--role`` and ``--slack`` set the affinity and the depth slack. A busy refusal names  |
|                                                                                                                                 |                           | what the name has still claimed                                                                                                                                                                                                                |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``release ID [--assignee NAME]``                                                                                                | claim graph               | hand the session node back unfinished: ready, generation moved; a node another conversation of this seat holds (a worker it started, or the one that started it) is released under that name                                                   |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``reap [--idle 6h] [--apply]``                                                                                                  | claim graph               | release claims idle past a window and mail each seat. Without ``--apply`` it prints what would happen. ``--idle`` is how long a claim may sit with no claim or tracker commit: ``6h``, ``90m``, ``2d``                                         |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``complete ID [--status done|failed|cancelled] [--gen G] [--assignee NAME]``                                                    | claim graph               | finish the session node; does not close the ticket; ``--gen`` and ``--assignee`` as on ``finish``; a node another conversation of this seat holds is completed under that name, and another seat's is refused                                  |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``cards [--dir DIR]``                                                                                                           | cards                     | print ``USER.md`` and ``MEMORY.md``                                                                                                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``approve REQUEST_ID``                                                                                                          | runtime                   | record explicit consent for one pending hook request; one matching attempt, fifteen-minute lifetime, deny rules still apply                                                                                                                    |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``policy [--fail-on-deny] ARGV...``                                                                                             | policy, pack              | the line as it would run, the trusted computing base (TCB) verdict if ``ljos-policyd`` answered, then a matching pack rule, then what the pack knows that bears on it; ``--fail-on-deny`` exits 1 on a deny, and without the flag the exit     |
|                                                                                                                                 |                           | stays 0                                                                                                                                                                                                                                        |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``hook [--limit N] [--prompt [--text]] [--fail-on-deny] [--runner NAME] [--git-commit] [--git-hook post-commit|post-rewrite]``  | policy, pack              | hook JSON or argv on stdin; ``--runner`` reads the payload of ``copilot``, ``gemini``, ``windsurf``, ``factory``, ``kiro``, ``cline``, ``qwen`` or ``crush`` and answers in that runner's shape; ``--prompt`` reads stdin as the prompt, and   |
|                                                                                                                                 |                           | ``--text`` with it prints what the agent would read as plain text instead of JSON; on ``PreToolUse`` the TCB and pack rules, a deny is ``permissionDecision``; ``--fail-on-deny`` exits 1 on a deny and without the flag the exit stays 0; on  |
|                                                                                                                                 |                           | a prompt the hits scoring at least 0.6 of the best that two scorers named, or with a pack that sends cosines the encoder's first hit at 0.66 or more and a hit sharing a word within 0.05 of the best cosine, preferences first then lessons   |
|                                                                                                                                 |                           | oldest first, each with its age, three at most, each once between compactions; on a failed tool, up to three that share at least two content words with the command and its error. ``--git-commit`` books the commit git just created and      |
|                                                                                                                                 |                           | reads no stdin; ``--git-hook`` names ``post-commit`` or ``post-rewrite``. Git hooks                                                                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``git-hooks install|uninstall|status``                                                                                          | git                       | install, remove, or show the global git hooks that book commits. ``install`` prints ``installed`` and the directory; ``uninstall`` prints ``uninstalled``; ``status`` prints where the hooks are and what ``core.hooksPath`` says. Git hooks   |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``bump-plan BUNDLE --project P --parent I [--generation G] [--dry-run]``                                                        | tracker                   | one child issue per module the bundle's lock builds, under ``I``, blocked by the modules built before it along the edges of its software bill of materials; ids are a hash of module and generation, so a rerun keeps what exists and adds     |
|                                                                                                                                 |                           | what is missing; ``--dry-run`` prints the rows                                                                                                                                                                                                 |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``findings STATE [--remember] [--all] [--issue I]``                                                                             | pack, deed store, tracker | the typed findings of an eb-stack campaign state, one per line: id, status, class/disposition, stage, recipe, the fix or the summary; ``--remember`` files one lesson per finding a person or a seat resolved (recipe, step, error line, fix), |
|                                                                                                                                 |                           | origin ``tool-echo`` at L0 (``derived``), under the recipe's name, its package and the class; ``ljos accept ID`` promotes it to ``explicit`` and writes it; ``--all`` takes the ones a later attempt superseded too; ``--issue`` cites the     |
|                                                                                                                                 |                           | state file as a deed on the issue                                                                                                                                                                                                              |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``sync [--key] [--import] [--export] [--adopt HOST [--force] [--dry-run]] [--adopt-legacy HOST]``                               | tracker, pack             | share the seat's memory across machines through the tracker repository: pull and take other machines' sealed logs, then write and push this machine's; ``--key`` prints this machine's age public key (made on first use) and stops;           |
|                                                                                                                                 |                           | ``--import`` only pulls; ``--export`` only writes and pushes. ``--adopt HOST`` reposts that host's live atoms as this machine's, origin ``peer``, in each log's scope, and drops that host's logs once every post and forget has landed.       |
|                                                                                                                                 |                           | A log committed within 14 days is refused unless ``--force``. ``--dry-run`` prints the plan and writes nothing. A fetch or merge that failed adopts nothing. Another machine retires that host's copies when it imports the adopt commit.      |
|                                                                                                                                 |                           | ``--adopt-legacy HOST`` reads HOST's logs from before the scope header: it pins the blobs filed under HOST now, and import takes only those bytes as HOST's until HOST writes a headed log.                                                    |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``merge-atoms CURRENT PATH``                                                                                                    | tracker                   | re-export this machine's atoms over a conflicted tracker log. git calls this as the ``ljos-atoms`` merge driver, ``%A`` then ``%P``                                                                                                            |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``handover --out DIR [--project P]... [--issue I]... [--to user@host:path] [--since-size N]``                                   | all                       | pack, seal, sign; ``--to`` copies the bag to another seat over ssh; ``--since-size`` puts the bridge from the size the receiver kept into the bag                                                                                              |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``receive DIR [--since KEPT] [--keep FILE] [--import]``                                                                         | all                       | check, and import the atoms with origin ``peer``; ``--since`` is the head ``--keep`` wrote at the last receive from this sender, and the bag's bridge must start there                                                                         |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``doctor [--all]``                                                                                                              | all                       | whether ljos is ready, and the rows that need action; exit 1 when a required part does not answer. ``--all`` (alias ``--verbose``) prints every row, the ones that answer included. A ``panes`` row names the tool that opens persona panes;   |
|                                                                                                                                 |                           | with ``jev.toml`` present, a ``jev`` row carries the month's calls, cached answers and spend                                                                                                                                                   |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``seat``                                                                                                                        | none                      | who is sitting: the seat this runner votes under, the holder this conversation claims under, and where the names came from                                                                                                                     |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``protocol``                                                                                                                    | none                      | print the sitting protocol                                                                                                                                                                                                                     |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``onboard [--harness NAME] [--skills DIR] [--dry-run] [--example]``                                                             | runner                    | alone, print the one ``mcpServers`` entry any runner takes; with a runner named in ``~/.config/ljos/harnesses.toml``, register ``ljos-mcp`` with it, install the protocol as its skill, and write the personas into its ``agents`` directory;  |
|                                                                                                                                 |                           | a shell runner (``shell = true``, ``grokbot``, or ``shell``) writes the skill and an env file and registers no server and no hook                                                                                                              |
+---------------------------------------------------------------------------------------------------------------------------------+---------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+

``ljos file "TITLE" --options A,B`` writes ``Options: A, B`` as the first body line.
``ljos options ISSUE A,B`` appends that line and refuses fewer than two.
Withdrawing a ballot writes ``ballot-withdrawn: VOTER HARNESS NONCE``.
The tally leaves that ballot out, and so does the push gate.
A later cast from that session says the ballot was withdrawn.

``send SEAT TEXT`` writes to one seat. A seat the pack has never heard
from is refused, with the seats it knows, since mail waits under the name
it was sent to; ``--new-seat`` writes to one that has not started yet.
``send --group NAME TEXT`` writes to the other members of that group. ``--interrupt`` leads the next prompt.
``--issue ID`` threads the message on that vissue issue, and the atom takes
the issue's scope so ``ljos sync`` carries it in that repository's sealed
log. ``inbox`` lists unread mail and writes no receipt. A pack that does
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
nothing. A shell writes the receipt before the hook exits. Cursor holds
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

``remember`` and ``prefer`` write origin ``user-declared``. ``finish --lesson``
files a proposal at L0, packset's ``derived`` level, with origin
``agent-derived``, and writes the lesson at once as ``user-declared`` when
the person types it in a terminal of their own with no agent runner
above the process. ``findings --remember`` files a proposal at L0 with
origin ``tool-echo``, because a campaign file is tool output. The prompt
hook files a correction as a proposal in the person's words, origin
``user-declared``, and does not write it. A proposal from tool output or
another seat (``peer``) stays at L0 until ``ljos accept ID`` promotes it to
``explicit`` and writes it. An agent's own lesson starts at L0, and the
same accept writes it. Accept keeps the origin. Under an agent runner
accept takes only a proposal another conversation filed, such as a
worker's. The writer records the acceptance. The same text typed with
``remember`` or ``prefer`` writes it as ``user-declared`` and satisfies the
proposal, and the held proposal is closed. An atom taken by
``receive --import`` or by ``sync`` has origin ``peer``, whatever origin the
sender wrote.

The claim graph records the held ticket too: a tracker id maps to one node
(FNV-1a 128 of the id) and a name to one actor; a 32-hex id passes through.

On Linux the pack client reads who owns the socket on its port from
``/proc/net/tcp``. A ``PACKSET_PORT`` that another user listens on is
refused. With nothing set and 8761 another user's, the seat uses port
18761 plus its user id modulo 10000 and exports it as ``PACKSET_PORT``, so
``packset ensure`` starts its writer there. ``PACKSET_URL`` is used as given.
Without ``XDG_RUNTIME_DIR``, the hud socket and seat records go in
``ljos-UID`` under the temporary directory, mode 0700, or in
``~/.cache/ljos/run`` when that name is another user's or a symlink.

User rules
==========

``ljos prefer --user TEXT --why REASON`` writes one user rule.
The file is ``$XDG_CONFIG_HOME/ljos/user-rules.json``, or ``~/.config/ljos/user-rules.json`` when that directory is unset.
Every seat on this machine reads it. The row does not decay and it is not reviewed, and the command does not also store a seat preference.
``--why`` is required with ``--user``.
``--promote ID`` copies that lesson into a user rule and leaves the lesson in the pack.
A promoted rule is stated ahead of a prompt only when the pack answers for that lesson within 400 ms, under the same id, stored as the person's; a pack that is slow, down or answers otherwise leaves it out of the prompt.
The row keeps a hash of the lesson's kind, text, ``ts``, ``valid_from`` and stored origin, and a record under that id that hashes otherwise, such as an id retracted and posted again, is another lesson; a rule promoted before the hash was kept is not stated until it is promoted again.
The read goes only with this seat's pack token, and only after the same read without it was refused, so a listener that answers without the token, or a packsetd run with ``PACKSET_AUTH=off``, confirms no promoted rule; a process of the same user that reads the token file can still answer as the pack.
``ljos rules`` lists a promoted rule the pack does not confirm apart from the user rules, as ``unverified`` with the reason.
``--forget ID`` leaves a tombstone so a later sync does not bring the row back.
``--sync`` pulls and pushes user rules through the tracker repository. Nothing leaves the machine unless that repository's ``.ljos/sync.toml`` names a scope. Then each machine's rules are sealed with age to the scope's recipients as ``.ljos/user-rules/<host>.json.age``, and a plaintext ``<host>.json`` this machine wrote before is removed. Plaintext files other hosts left are still read. A write and the session start run the same pass and follow ``LJOS_TRACKER_GIT``; ``--sync`` always pushes.
``--deny-mention HANDLE``, ``--deny-name NAME``, ``--deny-reply-to HANDLE`` and ``--deny-reply-name NAME`` may be repeated.
``ljos rule --user PATTERN --verdict deny|ask --why TEXT`` writes the argv law into the same file instead of the seat pack.
Without ``--user``, both commands keep writing the seat pack.

Git hooks
=========

``ljos git-hooks install`` writes a stub for every standard git hook and sets the global ``core.hooksPath`` to that directory.
The directory is ``$LJOS_GIT_HOOKS`` when that is set, otherwise ``$XDG_DATA_HOME/ljos/git-hooks``, otherwise ``~/.local/share/ljos/git-hooks``.
It prints ``installed`` and the directory.
A ``core.hooksPath`` that was already set is saved as ``previous-hooks-path`` in that directory.
When ``core.hooksPath`` still names that directory, ``ljos git-hooks uninstall`` puts the saved path back, or unsets it. It then removes the stubs and prints ``uninstalled``.
``ljos git-hooks status`` prints ``hooks:`` and the directory, or ``not installed``, then ``core.hooksPath:`` and the configured value, or ``(unset)``.
``ljos onboard --harness grokbot`` installs the same hooks.

Each stub chains to the repository's ``.git/hooks/<name>`` when that file is executable, otherwise to the saved path's hook of the same name, and keeps that hook's exit status.
``post-commit`` and ``post-rewrite`` also run ``ljos hook --git-commit --git-hook NAME``.
Booking never fails the hook. A line it prints goes to stderr, and a habitat that did not answer is a line in the book log.
``LJOS_OFF`` set to ``1``, ``true``, ``yes`` or ``on``, in either case, skips the booking, and the stub still chains.

``ljos hook --git-commit`` books the commit git just created.
``post-commit`` reads no stdin.
``post-rewrite`` reads the old and new shas and moves the citation onto the new commit. A dropped commit, an all-zero new sha, loses its citation.
``--git-hook`` is ``post-commit`` or ``post-rewrite``.
An amend or a rebase moves the deed onto the new sha and does not leave the replaced sha cited.
The sitting's lesson is written again for the new sha and closes the one that named the replaced sha.
A ``post-commit`` in the middle of a rebase or a merge does not book. ``post-rewrite`` does.
A commit already cited on an open sitting in that checkout is not opened as a second book.

The hook books as a seat. The process tree is not one.
The holder is ``SEAT@TAG``, the same name ``ljos claim`` uses for that conversation. ``LJOS_SESSION_ID`` alone is kept as written.
``LJOS_SEAT`` wins when it is set and is not ``seat``, ``you``, ``agent``, or a name that slugs to ``runner``.
Otherwise ``seats.toml`` is read through ``CURSOR_CONVERSATION_ID``.
The file is ``$LJOS_SEATS`` when that is set, otherwise ``$XDG_CONFIG_HOME/ljos/seats.toml``, otherwise ``~/.config/ljos/seats.toml``.
Each key is a conversation id and each value is the seat name.
An id with no row is ``grokbot-`` plus the first eight characters of the id.
With none of these, an ``ANTIGRAVITY_CONVERSATION_ID`` books as ``antigravity`` and an ``OPENCODE_SESSION_ID`` books as ``opencode``.
With none of these the commit stays unbooked.
A Claude Code or Codex session, named by ``CLAUDECODE``, ``CODEX_THREAD_ID`` or ``CODEX_SESSION_ID``, is not booked.
An opencode session is booked by this hook, including a commit from its shell or a subprocess.
An agy session is booked the same way when no seat was named.
Without these hooks, ``Stop`` and ``SessionEnd`` cite each commit HEAD's reflog says the checkout wrote since the sitting began, so a commit from a script, or one chained before a step that failed, still has a deed.
A pull, a merge or a checkout adds none.

The env file ``ljos onboard`` writes sets ``LJOS_GIT_HOOKS`` to ``${LJOS_GIT_HOOKS:-${XDG_DATA_HOME:-$HOME/.local/share}/ljos/git-hooks}``, the same directory the hooks install into.
``ljos doctor`` reports an env file that sets a different directory.

Commits under ``[git_hooks] skip`` in ``ljos.toml`` are not booked, and neither are ``/tmp``, ``/var/tmp``, ``/dev/shm``, the tracker (``VISSUE_ROOT`` or ``ISSUE_ROOT``), ``CLAIMDAG_DIR``, and the deedar store.
The table is read from ``$LJOS_CONFIG``, from ``./ljos.toml``, from ``~/.config/ljos/ljos.toml``, and from ``~/.config/ljos/seats/$LJOS_SEAT/ljos.toml``.
A reinstall rewrites the hook scripts and leaves that file alone.
``examples/ljos.toml`` shows the table.

MCP tools
=========

The seat serves these over the Model Context Protocol (MCP).

Writers: ``ljos_sitting``, ``ljos_finish``, ``ljos_calibrate``, ``ljos_persona``, ``ljos_playbook``, ``ljos_remember``, ``ljos_prefer``, ``ljos_forget``, ``ljos_trust``,
``ljos_learn``, ``ljos_graded``, ``ljos_island``, ``ljos_deed``, ``ljos_vote``, ``ljos_predict``, ``ljos_rule``, ``ljos_request_approval``, ``ljos_claim``,
``ljos_release``, ``ljos_complete``, ``ljos_consolidate``, ``ljos_findings`` (a reader without ``remember``), ``ljos_bump_plan`` (a reader with ``dry_run``), ``ljos_handover``, ``ljos_receive``, ``ljos_send``, ``ljos_read``, ``ljos_reply``, ``ljos_group``. Readers: ``ljos_search`` (with ``as_of``), ``ljos_conflicts``, ``ljos_inbox``,
``ljos_timeline``, ``ljos_due``, ``ljos_habit`` (a writer with a value), ``ljos_evidence``, ``ljos_current``, ``ljos_recall``,
``ljos_consensus``, ``ljos_brief``, ``ljos_personas``, ``ljos_playbooks``, ``ljos_cards``, ``ljos_policy``, ``ljos_doctor``. Resources:
``ljos://protocol``, ``ljos://cards/USER.md``, ``ljos://cards/MEMORY.md``. Prompts:
``start_a_sitting``, ``run_a_panel``, ``check_a_handover``. Every tool description opens with
when to call it.

By default the server lists the core set, in this order: ``ljos_rule``,
``ljos_request_approval``, ``ljos_prefer``, ``ljos_remember``, ``ljos_search``,
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

Environment
===========

=========================================================== ================================================================================================================================================================================
Variable                                                    Read by
=========================================================== ================================================================================================================================================================================
``PACKSET_URL``                                             the pack client; unset, ``http://127.0.0.1:8761`` (``PACKSET_PORT`` moves the port), or this user's own port when 8761 is another user's; ``off`` means no pack
``PACKSET_WORKSPACE``                                       the pack workspace; unset, ``seat``
``LJOS_SEAT``                                               override for who is sitting; unset, the seat is the MCP client that connected or the program above the shell, and the holder is that seat tagged with the conversation's process
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
``LJOS_PANEL_RUNNER``                                       the runner a decision panel's members run on, through its ``headless`` argv; unset, the seat's own runner, else Grok
``LJOS_PANEL_CONCURRENCY``                                  how many decision-panel members start at once; unset, the machine's parallelism clamped to 4; ``0`` starts every member; ``LJOS_MAX_PARALLEL`` is the same knob
``XDG_STATE_HOME``, ``XDG_CACHE_HOME``, ``XDG_RUNTIME_DIR`` where the Jev state, cache and runtime key live (see Jev settings); ``XDG_RUNTIME_DIR`` also keeps the hud socket and the seat records
=========================================================== ================================================================================================================================================================================

``~/.config/ljos/env`` is loaded for the keys above that the shell leaves unset.

A temporary HOME or working directory
-------------------------------------

ljos treats a ``HOME`` under ``/tmp``, ``/var/tmp``, ``/private/tmp``, ``/dev/shm`` or ``$TMPDIR`` as a test sandbox. There the command guard is off, and the memory writer does not start on its own either. A memory command then says so and names ``packset ensure``, which starts the writer by hand. A working directory under one of those, with a ``HOME`` outside them, is not a sandbox: ``cd /tmp && rm -rf ~/proj`` is judged. ``LJOS_GUARD=on`` keeps the guard on under a temporary ``HOME``, which is how a test suite exercises it; ``LJOS_OFF`` still switches it off. ``ljos doctor`` shows a ``guard`` row marked ``warn`` whenever the guard is off, from ``LJOS_OFF`` or from a temporary ``HOME``, the session start line says ``command guard off`` with the reason, and ``install.sh`` says so at the end of an install into a temporary ``HOME``.

A runner's table may also name ``start`` and ``resume``, the argv that open
a session in a persona's home and resume the latest one there; a
persona whose ``--runner`` is that table's name reasons in that session
(``ljos ask``, ``vote --jev``). ``hooks_named`` names the key a hooks file of
named hooks takes the seat's hooks under, each command told its event
with ``ljos hook --event``. A steps runner reads a permission answer that omits
``decision`` as a deny. The seat answers ``allow`` when no rule matches.

The pane script resumes a runner that exits non-zero, at most three times
a minute, and leaves one that exits 0. ``agents`` names the directory the
runner reads agent definitions from: ``~/.grok/agents``, ``~/.claude/agents``
or ``~/.cursor/agents``. ``headless`` is the argv a decision panel's member
runs, with ``{prompt_file}``, ``{prompt}``, ``{cwd}`` and ``{persona}`` filled;
when it is empty, the member runs ``grok`` with the seat's fallback flags.

``hooks_format = "cursor"`` writes ``hooks`` in Cursor's flat shape: ``version``
1, and one ``{command, timeout}`` entry an event. The seat registers the
permission check, the prompt, both tool results, compaction, stop and session end
there. Events ``~/.claude/settings.json`` already runs stay in that file.
``beforeShellExecution``, ``preToolUse``, ``beforeMCPExecution``,
``beforeReadFile`` and ``postToolUseFailure`` are still written here, and
the permission events fail closed, since Cursor runs that file's hooks
too and Claude's file does not name these. ``ljos doctor`` counts the
hook installed when those five are present and Claude's file carries
the seat hook, or when Cursor's file carries every event.

The hook reads a Cursor call off ``cursor_version`` in its stdin and answers
in Cursor's fields: ``permission`` on a permission check, with ``user_message`` and
``agent_message``; ``additional_context`` on a tool result; ``followup_message``
on a held stop. It still answers ``allow`` when there is no verdict, since
Cursor blocks the command when the answer to a permission check is not JSON. An ask on
``preToolUse``, which Cursor does not enforce, becomes a deny.
The IDE and the ``agent`` CLI read the same ``~/.cursor/mcp.json``.

A table with ``shell = true`` registers no server and no hook.
``ljos onboard --harness grokbot`` writes the skill and an env file beside
``harnesses.toml``. Source that file. ``LJOS_SEAT`` is the runner's name,
because a shell-only agent has no MCP client and the process tree is not
its seat. ``--skills DIR`` is where the skill goes.
``ljos hook --prompt`` reads stdin as the prompt.
``ljos hook --fail-on-deny`` and ``ljos policy --fail-on-deny``
exit 1 when the answer is a deny. Without the flag the
exit stays 0.

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
and a second group ``^mcp__.+__ljos_.+$`` for the seat's MCP tools),
once each, and drops it from events no longer listed. On the events
``hook_fail_closed`` lists, the hook carries ``"onFailure": "block"``, so the
runner stops the action when the hook fails, times out or cannot start;
the shipped ``claude`` table lists ``PreToolUse``, and ``onboard`` adds the field
to a hook written before. Claude Code reads it from v2.1.295. On ``SessionEnd`` the
hook fires the memories it injected during the session together and clears
the session's record. The hook reads the runner's JSON on stdin
(``hook_event_name``, ``tool_input.command``, ``prompt``) and answers
``{"hookSpecificOutput": {"hookEventName": ..., "additionalContext": ...}}``,
or nothing when the pack has nothing on the cue. On ``PreToolUse`` a TCB
or pack deny is ``permissionDecision`` ``deny`` and blocks. Plain text on
stdin is an argv line and answered in plain lines.

====================== ===============================================================================================================================================================================================================================================================================================================================================================================
Event                  The hook does
====================== ===============================================================================================================================================================================================================================================================================================================================================================================
``UserPromptSubmit``   answers with the memories the prompt activates, preferences first, each once between compactions; with Jev on, asks which claims bear on the prompt and whether it corrects the agent or puts a choice
``SessionEnd``         fires the memories injected during the session together and clears the session's record
``PostToolUse``        emits the held prompt note once, on the first tool result, for a runner that discards prompt-hook stdout (grok sends this event)
``PostToolUseFailure`` searches the pack on the failed command and its error and answers with up to three standing claims that two scorers named and that share at least two content words with the cue, each once between compactions
``PreCompact``         fires up to eight of the memories injected so far, drops all of them from the session's record so a later prompt can bring them back, and keeps a note naming the issue the conversation still has claimed, for the next delivery
``SessionStart``       after a compaction (``source`` ``compact``), on a runner that takes its context, answers with that note; a fresh session hears one line: that ljos is on, the memory and due counts, the command guard, and up to three preferences
``PreToolUse``         applies the TCB and pack rules to the command (matcher ``Bash|Edit|Write|MultiEdit|NotebookEdit|apply_patch``); a deny blocks. It keeps the call's ``agent_id`` for each ``ljos`` line, and on a seat MCP tool (``^mcp__.+__ljos_.+$``) only notes a subagent's call, so the ballot binds the session plus that agent
``Stop``               speaks only when the turn ran no tool; blocks one turn that was asked for a decision and cast no ballot; with no issue held, blocks one turn that used tools and never touched the seat; with Jev on, audits the turn once from the runner's transcript; then holds one turn for unread interrupt mail (``decision: block``, or ``followup_message`` in Cursor) and receipts it
``SubagentStop``       names the issue the subagent's conversation has claimed and asks for the ballot on a decision; with Jev on, audits the turn once
====================== ===============================================================================================================================================================================================================================================================================================================================================================================

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

What a rule reads
=================

A rule is tried on every command a line runs, the first and the rest. The
line is split on ``&&``, ``||``, ``;``, ``|``, ``&`` and new lines
outside quotes, and each piece is read again inside: the bodies of
``$(...)``, backticks, ``<(...)``, ``>(...)`` and subshells; the script of
``sh -c``, ``eval``, ``env -S``, ``trap``, ``alias``, ``su -c``, ``script -c``,
``flock -c`` and ``nix-shell --run``; a here-string or here-document fed to a
shell, and ``echo`` or ``printf`` piped into one; the commands of ``find
-exec``; and the line ``ssh`` runs on its host. Leading ``NAME=value`` words,
wrappers (``sudo``, ``doas``, ``env``, ``time``, ``nohup``, ``exec``, ``command``,
``nice``, ``ionice``, ``timeout``, ``setsid``, ``stdbuf``, ``xargs``, ``flock``,
``chronic``, ``unbuffer``, ``watch``), ``case`` patterns and function
definitions come off, and quotes, ``$'...'`` and escapes come off each
word, so ``git 'push'``, ``$'git' push`` and ``git -c alias.p=push p`` read as
``git push``. A line nested more than eight deep is refused whole.

A rule reads the line as written. It does not see a command whose name
comes from a variable (``$g push``), a script run from a file, an alias or
git alias defined in an earlier call, a recipe in a Makefile or justfile,
or a call made from inside another language (``python -c``, ``perl -e``). A
rule stops an agent's mistakes; it is not a sandbox. For a push that
must never happen, protect the branch on the forge.

Push checks
===========

An asked ``git push`` (a seat rule ``git push*`` with verdict ``ask``) is checked
by where it goes; a deny stays a deny. The forge says whose the remote
is: for GitHub, ``gh api`` reports the person's push permission, whether
the owner is their own account, the collaborators and the releases. On a forge the seat cannot ask, the seat counts a personal namespace
that carries the person's GitHub name as theirs. What ``gh`` says is remembered in the pack as a standing claim on
the entity ``repo:OWNER/REPO``, so the next push reads the pack and not
the forge, the hook raises the claim when a command names the
repository, and the review clock brings it back. A wrong claim is
forgotten with ``ljos forget ID``, and the next push asks the forge again.

+---------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| Push                                                                                                                | Needs                                                                                                                                                                                                                                                                                 |
+=====================================================================================================================+=======================================================================================================================================================================================================================================================================================+
| a branch, to the person's own account, no other collaborator, no releases or tags                                   | nothing; it runs                                                                                                                                                                                                                                                                      |
+---------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| a branch, where the person can push but the repository has releases, other collaborators, or an organisation's name | ``LJOS_CITE=ISSUE`` where the seats that voted on ``ISSUE`` agree and one of them is not the seat pushing (persona and Jev ballots neither pass nor block it; open or closed does not matter); a deed cited alone is refused, since the pushing seat can mint one; the pass is noted  |
|                                                                                                                     | on ``ISSUE``                                                                                                                                                                                                                                                                          |
+---------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| a remote the person cannot push to, ``--mirror``, deleting a remote branch or tag, a force                          | the person runs it                                                                                                                                                                                                                                                                    |
+---------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| a tag (``--tags``, ``--follow-tags``, ``refs/tags/``)                                                               | nothing; it runs, as a branch push does                                                                                                                                                                                                                                               |
+---------------------------------------------------------------------------------------------------------------------+---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+

The remote is the one the command names, else the branch's upstream,
else ``origin``, read in the directory a ``cd`` or ``git -C`` before the push
names.

Sync files
==========

=============================================== ==================================================================================================================
File                                            Contents
=============================================== ==================================================================================================================
``.ljos/sync.toml`` (in the tracker repository) the repository's scope and its age recipients; a ``[projects]`` table sends one project's lessons to another scope
``.ljos/atoms/``                                one sealed log per machine
``.ljos/user-rules/``                           one file of user rules per machine, sealed to the scope's recipients (``<host>.json.age``); a plaintext ``<host>.json`` is from before the seal
=============================================== ==================================================================================================================

Jev settings
============

``~/.config/ljos/jev.toml``. The file is absent or ``enabled = false``: Jev is off.

=================== ======================================== =============================================================================================================================================================================================================
Key                 Default                                  Meaning
=================== ======================================== =============================================================================================================================================================================================================
``enabled``         ``false``                                turn Jev on; the call sends the prompt and claims off the machine
``backend``         ``jev``                                  which judge answers: ``jev``, ``chat`` (one JSON-mode chat completion at ``endpoint`` plus ``/chat/completions``, model ``model``) or ``command`` (the ``command`` argv, request on stdin, answers on stdout)
``command``         unset                                    the argv of the ``command`` backend; it prints ``{"answers": ...}`` inside ``budget_ms``
``key_file``        unset                                    file holding the key, one line, mode 0600
``key_cmd``         unset                                    command that prints the key on its first line, as an argv list; asked once per login
``model``           ``jev-1.13.0``                           model name; ``typesafe/jev-1.13`` on OpenRouter
``budget_ms``       2000                                     how long the hook waits for the answer
``endpoint``        ``https://api.typesafe.ai/v1/systemone`` API endpoint; ``https://openrouter.ai/api/alpha/decisions`` for OpenRouter; the base URL (``.../v1``) for ``chat``
``monthly_usd``     4.0                                      the month's spend in US dollars past which the hook stops asking
``min_words``       4                                        a prompt with fewer words is skipped
``min_candidates``  2                                        fewer candidate claims than this is skipped
``usd_per_mtok_in`` 0.042 on ``jev``, required on ``chat``   US dollars per million input tokens, for an API whose answer carries no cost. A ``chat`` judge with no price is not asked
``bears_at``        0.5                                      probability at which a claim bears on the prompt
``cue_at``          0.5                                      probability at which the prompt counts as a correction or a choice
``escalate_below``  0.8                                      a persona ballot under this confidence goes to a subagent
``cache_days``      7                                        days an identical request is answered from the cache; 0 turns it off
``key_env``         unset                                    environment variable holding the key
``[judges.NAME]``   none                                     a further judge: ``backend``, ``command``, ``key_file``, ``key_cmd``, ``key_env``, ``model``, ``endpoint``, ``budget_ms``, ``usd_per_mtok_in``, and ``weight`` (1.0) in a pool
``[route]``         every decision to ``default``            which judges answer each decision: ``prompt``, ``ballot``, ``audit``, ``review``, each a list of judge names; ``default`` is the judge the top-level keys describe
=================== ======================================== =============================================================================================================================================================================================================

====================================== ========================================================================================================================================
Path                                   Contents
====================================== ========================================================================================================================================
``$XDG_STATE_HOME/ljos/jev-cost.toml`` the month's calls and spend
``$XDG_STATE_HOME/ljos/jev-log.jsonl`` every Jev answer: claim ids, a prompt hash, latency, timeout or failure, backend and model. ``ljos judge-score`` joins it with outcomes
``$XDG_CACHE_HOME/ljos/jev``           the answer cache: a hash of the request and the reply, directory mode 0700, file mode 0600. The prompt and the claim text are not stored
``$XDG_RUNTIME_DIR/ljos/jev-key``      the default judge's key from ``key_cmd``, mode 0600; ``jev-key-NAME`` for a named judge
====================================== ========================================================================================================================================

The four routes are ``prompt``, ``ballot``, ``audit`` and ``review``. ``backend = jev`` posts to ``endpoint`` (TypeSafe's API, or OpenRouter at ``https://openrouter.ai/api/alpha/decisions`` with model ``typesafe/jev-1.13``). ``backend = chat`` posts one JSON-mode completion to ``endpoint`` plus ``/chat/completions`` and must set ``usd_per_mtok_in``. With ``backend = command``, the request stays on the machine. TypeSafe's terms say they will not train on customer data. They keep a perpetual licence to derive telemetry from inputs. Zero data retention needs an enterprise agreement and is off by default. This seat does not train on Jev's answers. A judge ballot is a trust row with ``to`` ``judge:MODEL``, and ``learn`` updates it.

The learning rules
==================

``learn`` adds the outcome to each voter's record of hits and misses. A
voter's accuracy is read off its record and shrunk toward the panel's
pooled accuracy by empirical Bayes (Efron and Morris,
doi:10.1080/01621459.1975.10479864). The prior weakens as the records
spread beyond binomial noise; the first outcome leaves every voter alike,
and a long record keeps the differences it shows.

The weight every other voter gives a voter is the log odds of that
accuracy, Nitzan and Paroush's weight (doi:10.2307/2526438); the best
voter weighs one and none weighs less than 0.01. A panel with nobody above
chance weighs every voter 0.01. Rows are written complete, so the settle
sees the whole graph, and each carries its voter's record. ``learn`` also
writes an ``outcome`` atom, the issue and the option it closed on.

``--rule hedge`` runs the multiplicative weights rule of Hedge
(doi:10.1006/jcss.1997.1504) instead. Every voter whose ballot the outcome
refuted shrinks in every other voter's row by ``beta`` (default 0.5),
floored at 0.01. A vindicated voter keeps its weight, and a missing row
starts at 1.

``consensus`` reads each issue's ballots beside the option it closed on once
five issues have an outcome, and asks ``ljos-consensus correlation`` how far
the voters share their mistakes. A pair counts only when its correlation
passes the one-sided test of independence at five percent. A voter's
inbound weight in the settle is multiplied by
``1 / (1 + the sum of its counted correlations)`` (``settle --discount-of``);
exact clones count as one voice under it if their record has a hit and a
miss, and a looser cluster as more than one. A line before the settle
names the outcomes read, the independent voices the panel has and each
voter discounted. Nothing is passed or printed when no pair counts.

A row carries the domains it is scoped to. An unscoped row applies to
every issue; a scoped row applies when one of its domains is a word of the
issue's title. ``learn`` writes its rows scoped to the entities of the island
the issue's title activates, eight at most, so a voter refuted on one topic
keeps its standing on the rest; a scoped learn starts from the unscoped row
when it has none of its own and leaves the unscoped row standing. When
both apply, the scoped row is the one the settle sees. ``--share S`` then lifts
every row by ``S`` of its distance to one, the fixed-share rule of Herbster
and Warmuth (doi:10.1023/A:1007424614876). A voter refuted long ago is not
held down forever, and the best voter can change. The default is zero,
plain Hedge. An outcome also
moves the anchor of every persona it refuted toward one by ``1 - beta`` of
the gap: a persona that keeps being wrong listens more.

A persona is a ``persona`` atom: a name, an anchor in [0, 1], a view, and the
domains it speaks to. ``consensus`` passes the anchor of each persona on the
ballot to both settles as ``--susceptibility-of``, and gives every other voter
on that ballot, the seat included, an anchor of 0.5. A ballot with no
persona settles as plain DeGroot. At 1 a persona keeps none of its
ballot. An anchor below 0.5 settles at 0.5: under Friedkin-Johnsen with
equal trust, the share of its ballot a voter keeps is its weight, and a
stubborn voter pulls the rest toward it (Acemoglu, Como, Fagnani and
Ozdaglar, doi:10.1287/moor.1120.0570). A persona at 0 would count as two
seats and tie a 2-to-1 vote on its own. Trust rows still weigh voters as
they say. Before the default, the seat sat at 1, so any anchored persona
took the whole settle. The settle prints in words: the shares, who leads
and by how much, polarization, each voter's influence, the share of its
own ballot each voter keeps. Above the settle, ``consensus`` prints what a
push cite on the issue decides: the option every seat ballot names and the
seat ballots it counted, or why it decides nothing, with the persona and
judge ballots it left out. The settle below is advice and decides no push.
When trust rows exist, the standing beside the settle names the voters on
this issue; the other names in the rows are counted, and ``--json`` lists
them.
``consensus ID --json`` prints one JSON document, and nothing else on
stdout. The push cite stays at the top level under ``push_cite``, with the
keys it already uses (``issue``, ``pusher``, ``stands``, and ``choice`` with
``counted``, or ``refusal``). The model crate's settle is under ``settle``.
``surprising`` and ``reputation`` are present when those readings ran.
``consensus ID --compare`` also runs ``vissue consensus``, the tracker's
settle, and that output is under ``tracker`` in the same document. The
tracker also runs when ``ljos-consensus`` is not installed. Without
``--compare``, and with the model crate installed, the document has no
``tracker`` key. An issue with no ballots is refused before any settle
runs, in the command and in the MCP tool, and the message says how to
cast one (``ljos vote ID --for OPTION``, or ``ljos panel ID``).
A vissue with ``--file-dissent`` files dissent children only under
that flag; an older one files them on every run.

A ballot's ``--confidence`` is recorded and weighs no settle and no push
cite. ``consensus`` says on how many ballots it was stated. ``learn ID
--outcome OPTION`` scores it once the outcome is known: Brier and log score
and calibration per voter.

Without an outcome, ``calibrate`` estimates each voter's accuracy from the
project's issues with two or more ballots, by expectation maximisation
over the items' hidden answers (Dawid and Skene, doi:10.2307/2346806). It
then writes the log odds of each accuracy as the weight every other voter
gives that voter. The most reliable voter weighs one, and no weight
falls below 0.01.

Crates
======

================== =========================================================================
Crate              Carries
================== =========================================================================
``ljos-cli``       the library and ``ljos``
``ljos-mcp``       the MCP server
``ljos-consensus`` DeGroot and Friedkin-Johnsen settles, Seldon export (separate repository)
================== =========================================================================
