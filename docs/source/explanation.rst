:description: This page describes ljos 0.35.0. Items marked "coming in 0.35.0" are not in 0.34.1.
:cover: https://leidarljos.github.io/ljos/_static/og.png

This page describes ljos 0.35.0. Items marked "coming in 0.35.0" are not in 0.34.1.

One seat over the habitats
==========================

|image1|

ljos keeps nothing of its own. Each kind of fact lives in one store. ljos calls that
store's command line to read or write it.

The aim is plain. Several agents work on one project, and they should reach agreement and
work things out between them. They need a place to say what the work is, who holds it, and
what each of them thinks. They also need something underneath that stops a dangerous command
before it runs. The first is the seat. The second is ``ljos-policyd``.

====================== ======================================================== ============================================== ============================= ==================================================== ===========================
Store                  Keeps                                                    Lives                                          Lasts                         Written by                                           Look at it
====================== ======================================================== ============================================== ============================= ==================================================== ===========================
tracker (vissue)       tickets, notes, votes, the deeds a ticket cites          org files in a git repository                  as long as the repository     ``ljos file``, ``note``, ``vote``, ``finish``        ``vissue show ID``
pack (packset)         memories: lessons, preferences, personas, trust          the ``packsetd`` daemon's store                until superseded or forgotten ``ljos remember``, ``prefer``, ``accept``, ``learn`` ``ljos search WORDS``
deed store (deedar)    signed records of what a piece of work produced          a content-addressed directory                  permanent                     ``deedar create``                                    ``ljos evidence ACCESSION``
claim graph (claimdag) who is working on what right now, as a lease per sitting ``$XDG_RUNTIME_DIR/claimdag``, one per machine until reboot                  ``ljos sitting``, ``finish``, ``release``            ``claimdag list --all``
consensus              the weighting that settles a vote                        computed from the tracker and the pack         nothing stored                ``ljos consensus``                                   ``ljos consensus ID``
====================== ======================================================== ============================================== ============================= ==================================================== ===========================

The claim graph shows itself least, by design. It answers one question while work runs: is
anyone already on this ticket, here, now? ``ljos sitting`` takes the ticket's node before it
returns. A second sitting on the same ticket learns who has it. ``ljos finish`` marks the node
done. A reboot clears the store, and a lease never outlives its holder.

A deed accession is the one identifier the writable stores share. The tracker cites it on a
ticket, and the pack cites it in a memory. The deed store answers for it.

The contracts
=============

- Citing a deed names it. The bytes stay in deedar.
- A finish ends the sitting. A person closes the ticket when the work is accepted.
- A finish hands the claim back and puts the ticket in the state it had before the claim.
- Cards are read-only. The seat writes to the pack, and a person writes the cards.
- A tool that fails is a store refusing or down, and it says which. It is never an empty answer.
- ljos calls no model, unless the person turns on Jev.

How a shell command is judged
=============================

Before an agent's shell command runs, the runner's hook hands it to ``ljos hook``. ``ljos hook``
asks ``ljos-policyd`` and relays the answer. policyd is the one place that decides whether a
command is dangerous. It is the trusted part. ljos is not.

An earlier ljos ran nine checks of its own before it asked policyd. Each check came from a
real incident, and each one refused work it should not have. A parser cannot stop a hostile
process of the same user, and it can refuse a harmless command. So 0.35.0 drops those checks.
Two remain, because only ljos knows the tracker and the sitting. They are described below.

Four answers
------------

policyd answers allow, ask, deny or rewrite. The first deny wins. An ask goes to the person. On
runners that have a permission prompt, the hook answers ``ask`` and the runner shows its own prompt.
The person approves or refuses there.

It reads the command as a shell does. It looks through ``sh -c``, ``env``, ``timeout``, ``xargs`` and
``nohup``. It follows ``$cmd``, pipes and quotes. It does not run a shell, and it does not read the
scripts a command runs.

The built-in law covers:

- privilege: ``sudo``, and ``chmod`` that sets setuid
- code from the network: ``curl`` or ``wget`` piped to a shell, and decoded text piped to a shell
- raw disks: writes with ``dd of=/dev/...``, ``mkfs`` and ``wipefs``
- deletes: ``rm -rf`` outside the work tree and ``/tmp``, and recursive ``chmod`` or ``chown`` on system trees
- shared history: a force push to ``main``, ``master``, ``develop`` or a protected branch
- published refs: deleting a remote branch, ``git filter-branch``, ``git filter-repo``, ``git reflog expire``, and deleting a published ref
- uncommitted work: ``git reset --hard``, ``git clean -f``, ``git checkout .`` and ``git stash clear``, as one ask
- credentials: reading a named credential file, or sending one over the network
- its own rules: writes to the hook files, the policy files and the ljos programs
- mail: outbound mail whose From is not the person's own address

An ordinary push of a branch runs. A lease push to a feature branch runs. A lease push onto a
protected branch asks.

The rewrite
-----------

Coming in 0.35.0. Some commands are not dangerous. They are wrong for this project. Refusing
them costs a round trip, and the agent often retries the same line.

So policyd can answer with the fixed command. The answer is one line: ``allow``, then ``rewrite``, then
the new command, separated by tabs. Its first rewrite runs the command through ``uv run``.

Claude Code and Cursor accept an updated command from the hook, and ljos hands the new line back
through that field. A runner without that field gets a note that names the new command. policyd
writes a log of what it denies. There is no log of rewrites yet, and no per-repository switch to
turn one off.

The two checks ljos keeps
-------------------------

- The tracker is written through ljos. A command, or a file tool, that writes the tracker's
  files by hand is refused. Ticket text is data, so a ticket body that mentions a path or a
  command is never read as one.
- A push onto a protected branch asks when the ticket of the current sitting has no settled
  consensus. It asks. It does not deny. With one seat, there is no consensus to find, so it asks
  every time and you answer.

What fails open
---------------

If ``ljos-policyd`` is missing, slow or crashes, the command runs and ljos logs a warning.
Stopping every tool call because a checker was busy cost more than it saved. ``ljos doctor``
shows the policy row, which says whether policyd runs its own table or phronesis after that table. ``ljos policy -- COMMAND`` prints the answer for one command without running it.

What policyd is not
-------------------

policyd reads tool calls. It is not a sandbox. Code that runs as the same user, outside a tool
call, can write what policyd would refuse. The files it guards are safer if they are owned by
root. Treat it as a way to stop an agent's mistake, and use the operating system to stop an
attacker.

Cloud seats
-----------

On a cloud seat, one policyd rule refuses ``gh pr create`` and ``gh pr merge``. The same rule refuses
a push to any ref but the seat's own branch. Branch protection on the remote does the rest.

Who is sitting
==============

The seat is the name an agent works under. ``LJOS_SEAT`` sets it. When it is unset, ljos derives a
name from the runner, and a derived name is a seat. ``ljos-mcp`` names the seat after the client that
connected. ``ljos seat`` prints the seat, the holder, and where each name came from.

Claims belong to one conversation. Memory, ballots and trust belong to the seat. The holder
is ``SEAT@session``, and the session id is the one the runner gives.

A seat does not need to be listed before it writes. An unset seat writes under its login seat with
a warning. The tracker keeps the list of seats.

- ``ljos seat add NAME`` registers a seat on the tracker. Coming in 0.35.0.
- A seat grant lets one named seat do one thing it could not do alone, such as close a ticket.
  Coming in 0.35.0.
- Without a grant, ``ljos finish`` leaves the ticket open, and a person closes it.

A shell-only agent has no MCP client and no hook. ``ljos onboard`` for such a runner writes the skill
and an env file. Source the file, and ``LJOS_SEAT`` is the name.

Nothing else needs a variable. ``ljos onboard`` makes the deed store and the tracker, and starts the
pack writer. ``ljos offboard`` removes what onboard wrote. It is coming in 0.35.0.

What happened
=============

Several seats write to one tracker, so the question "what happened while I was away" needs
one answer. Today it takes several: ``ljos timeline ID``, ``ljos claims`` and ``ljos log --seat NAME``.
``ljos digest`` is coming in 0.35.0. It reads the tracker and reports, for each seat, what it
filed, claimed, voted on, noted and finished. It writes nothing.

Agreement that learns
=====================

A tally counts, and a count is right only when every voter is worth the same. The seat settles a
vote with DeGroot's model (doi:10.1080/01621459.1974.10480137). Friedkin and Johnsen's anchored
version (doi:10.1080/0022250X.1990.9990069) is the other option. Both run over trust rows. The
rows are pack atoms, so they are dated, they can be replaced, and they export.

``ljos learn`` moves the rows by what turned out right. Each voter has a record of outcomes that
agreed with its ballot and outcomes that did not. The rows are the log odds of that record. A
voter is weighed by what it got right, not by how often it was punished.

The multiplicative update of Hedge (doi:10.1006/jcss.1997.1504) is kept as ``--rule hedge``. On
the consensus crate's synthetic voters, the record rule answered 0.929 of the questions. Hedge
answered 0.831, and a plain count answered 0.820. The crate has the setup: nine voters, four
hundred questions, twenty seeds.

When nobody names an outcome, ``ljos calibrate`` estimates each voter's accuracy from the project's
history (Dawid and Skene, doi:10.2307/2346806). It needs no truth labels.

Agents that run on one model share its mistakes. ``learn`` keeps the option each ticket closed on.
After five tickets have one, ``ljos consensus`` asks which voters err together and discounts them.
A short record also misleads, so ``learn`` shrinks each accuracy toward the panel's pooled accuracy
(Efron and Morris, doi:10.1080/01621459.1975.10479864).

A persona is a voter with a view, an anchor and the domains it speaks to. A panel seats the
personas whose domains the ticket speaks to. Each persona casts its own ballot before it reads
the others. Chen et al. (doi:10.48550/arXiv.2403.02419) show that a count does not improve with more
voices on hard items. A weighted settle is the alternative this seat takes.

Ballots from personas and from Jev are advice.

Jev
===

Jev is TypeSafe's decision model. It is the one place a model enters the seat, and it is optional.
It stays off unless ``~/.config/ljos/jev.toml`` says ``enabled = true``. A call sends the prompt and up
to ten candidate memories off the machine, so the machine has to allow that.

Jev does not judge commands. policyd does. Jev answers four kinds of question that have a local
answer already:

====== ====================================================================== =============================================================
Point  The question                                                           The local answer
====== ====================================================================== =============================================================
prompt which memories bear on this prompt, and is it a correction or a choice the encoder's cosine or a lexical match, and two phrase lists
ballot which option does a persona vote for, and what will the others pick    one subagent per persona
stop   has the agent claimed done beside a failing test                       rules on the held ticket
review is a memory that is due still true                                     the person grades it
====== ====================================================================== =============================================================

Jev answers with a probability and writes no text. So it replaces the answer at each point and
leaves the structure alone. If Jev does not answer, the local answer stands. A machine without
``jev.toml`` runs as before.

A ballot Jev casts is recorded as ``judge:MODEL``, not as the persona. The probability of the chosen
option is its confidence. ``learn`` updates the trust row of that judge, so Jev's calibration on
this seat's questions becomes a measurement. ``ljos judge-score`` reads the log.

Personas on one model are one voter. So a panel through Jev casts one ballot when every seat is
sure and all agree. A split or unsure panel goes to subagents, and the paid model is used only for
the contested question. That is a cascade in the sense of FrugalGPT (doi:10.48550/arXiv.2305.05176).

Every answer goes to ``jev-log.jsonl`` with the claim ids, a hash of the prompt and the latency. The
log does not keep the prompt. Nothing here trains on Jev's answers.

Memory that grows, is reviewed, decays, and is retracted
========================================================

|image2|

A claim enters the pack because someone said to keep it. It enters a review clock at the same
moment. The clock is the spaced-repetition model that the Free Spaced Repetition Scheduler fits to
review data (doi:10.1145/3534678.3539081). It tracks a stability in days, a difficulty and a due
date. A recalled claim grows more stable, and a lapsed claim halves. ``ljos due`` lists what the
seat is about to forget.

When the pack's decay slot is on, retrievability scales a search score, so an unreviewed claim
sinks without vanishing. On a corpus where one claim per topic stays recalled, retrievability ranks
the kept claim first 0.947 of the time. Lexical scoring is at chance, and a recency half-life gets
0.270. The packset site carries the table.

A claim shown wrong is retracted with the deed that showed it. A contrary claim closes the old
claim's window. The record that it was held stays.

What writes the pack
--------------------

``remember``, ``prefer``, ``trust``, ``learn``, ``graded``, ``forget``, an imported handover, and a session-end
read when the conversation has an open sitting. At ``SessionEnd``, the hook keeps up to five sentences
that carry a cue word, from the person's and the agent's prose. It writes each as an unverified note
on that sitting's ticket. With no sitting, it writes nothing. The prompt hook never injects those
notes. ``LJOS_EXTRACT=off`` turns the read off.

A lesson an agent proposes at ``finish`` is a proposal. ``ljos accept`` writes it as a standing lesson,
and the person runs ``accept`` in a terminal of their own. A standing choice the person states in a
prompt is saved as a preference in their words. "From now on use bun, never yarn" is one.

When the seat speaks
--------------------

The hook speaks at a prompt, after a failed tool and at a stop. It fires its memories before a
compaction. A prompt gets what it activates, three claims at most. A claim reaches a session once
between compactions. It arrives when it first matters, not on every command after.

A runner that reports a failed tool gets a search of its own on the command and its error. A
compaction drops what the seat handed over, so the next delivery names the open ticket and where its
working set is.

Islands
-------

|image3|

A task does not touch everything a seat knows. The pack links each claim to the claims it shares
names with. ``ljos island`` finds the cluster a task activates. The top search hits are the seeds,
activation spreads two hops along the links, and the cluster comes back strongest first. That is
spreading activation over a semantic network (Collins and Loftus, doi:10.1037/0033-295X.82.6.407).

When the seat goes on to use an island, ``ljos island --fire`` says so. The strongest eight fire
together, and the weight of each link between them grows. Links nobody walks fade without being
deleted. That is Hebb's rule with Oja's forgetting term (doi:10.1007/BF00275687).

The island lost as a ranking on LongMemEval. Activation flows to well-connected claims, and a
question asks for something else. So the island is what the seat prints beside the hits for
orientation, and what fires together after use. It is never the order the hits come in.

Time as data
------------

Every record is dated. An atom carries the writer's clock and the window it was live in. A deed
carries the time it was produced. The tracker's logbook carries the time of each note, state change
and claim. The seat hands the reader that as data. Each recalled memory carries its age in words,
such as ``today`` or ``3 weeks ago``. ``ljos timeline ID`` merges the three stores into one dated list.
``ljos search --as-of TIME`` asks the pack as it was at an earlier time.

A reader is a poor calendar. A store that knows every date is a good one. So the store does the
arithmetic, and the reader reads the order.

Handover that can be checked
============================

|image4|

A handover is a BagIt bag. It holds the tracker slice, the pack's atoms, and the deeds both
cite. Each deed carries its inclusion receipt against the log head. The manifest is signed when the
sender has a key.

The receiver checks three things in order. The bag arrived as written. The deeds were in the
sender's log before the handover. A key the receiver accepts signed it. Only then are the atoms
imported, trust rows included. Learning travels with its evidence.

The post-commit note
====================

``ljos git-hooks install`` adds one post-commit hook. On the first commit, it opens
the sitting or names the one already open. It notes the commit's SHA on the held ticket. It never
refuses a commit.

Model and runner agnostic, human readable
=========================================

ljos runs without a model unless Jev is on. The stores are files a person can read: Org headings,
one JSON object a line, a content-addressed directory. Any agent that can run a command or call a
Model Context Protocol tool can work the seat. A person can do the same from a shell or an editor.

The seat does not know the runners. It knows one thing every runner does: it connects and says its
name. A new runner needs no table anywhere. ``ljos onboard --example`` lists the shapes ljos knows.
A runner it does not know can be described once in a runners file under ``~/.config/ljos``.

The agent is told how, in one text. ``ljos protocol`` prints it. ``ljos onboard`` installs the same text
as a skill, and the server serves it at ``ljos://protocol``. It says which store answers which
question, the order of verbs in a sitting, and the refusals.

The pack's token
================

The seat sends its pack token only after two refusals from the address the writer's URL resolved
to. The first has no token. The second has a token of the same shape that is not this seat's. Both
have to answer 401. A process of the same user can read the token file, so this check cannot tell
that process from the writer. It stops a stranger that took the port.

Where it sits among memory systems
==================================

Mem0, Zep, MemGPT, A-MEM and HippoRAG put a model in the write path. A model reads a conversation,
extracts facts, and decides what to add, update or delete. ljos does not. What it keeps as a belief
is what someone said with ``remember`` or ``prefer``, stored as written.

That choice has a cost. A model that reads a transcript finds facts nobody said to remember, and on
chat-log benchmarks that coverage is most of the score. The pack has a proposals path for it, and a
proposal becomes a claim only on an explicit accept.

Forgetting here is by review, not by age. Time is data, as in Zep. Retrieval is a panel of scorers
fused together, and every hit says how many scorers named it. Above the pack, the seat adds what a
memory alone does not have. Votes settle under trust that learns. Deeds record what the work
produced. A claim graph says who holds what. A handover is one another seat can check.

.. |image1| image:: _static/seat.svg
   :width: 100.0%
.. |image2| image:: _static/memory.svg
   :width: 100.0%
.. |image3| image:: _static/island.svg
   :width: 100.0%
.. |image4| image:: _static/handover.svg
   :width: 100.0%
