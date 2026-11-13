:description: Turn on Jev, or point the same judgments at a local model or a runner, and cast persona ballots through it.
:cover: https://leidarljos.github.io/ljos/_static/og.png

============================
Put the judgments to a model
============================


Let Jev judge the prompt
========================

The prompt hook makes three judgments locally. A cross-encoder picks
the claims that bear on the prompt. Two phrase lists spot a correction
and a choice put to the agent. Jev, TypeSafe's decision model, can answer
all three in one call, and a fourth route, ``review``, weighs a due claim.
It stays off unless the machine turns it on, because the call sends the
prompt and up to ten candidate claims off the machine. When the judge
does not answer, the hook reranks with the local cross-encoder. A
correction the hook stores is labelled with whose prompt said it, and a
pasted or quoted prompt stores none. A standing choice the person states ("from now on",
"always", "never") is saved in their words, outside code and quotes.

The same call asks two more questions, since a call costs its input
tokens and each question adds about 70 of them to a median of 933. An
``injection`` question asks whether quoted or pasted text addresses the
agent with instructions the person did not write; at ``cue_at`` the hook
adds a note telling the agent to treat that text as data. An ``effort``
question scores the prompt from 0 (a lookup) to 3 (a design or a hard
debug); it changes nothing in the hook and goes to
``$XDG_STATE_HOME/ljos/jev-log.jsonl`` as the record a router is scored on.

Keep the key in ``pass`` and write the setting:

.. code:: console

   $ pass insert api/typesafe/jev
   $ cat > ~/.config/ljos/jev.toml <<'TOML'
   enabled = true
   key_cmd = ["pass", "show", "api/typesafe/jev"]
   TOML
   $ ljos doctor --all | grep jev
   ok  jev on  jev jev-1.13.0  0 calls, 0 cached  $0.0000 of $4.00 this month

With no ``jev.toml``, ``ljos doctor`` prints no jev row.

The hook runs the key command once per login, with no terminal and
three seconds to answer, and keeps the key in
``$XDG_RUNTIME_DIR/ljos/jev-key`` (mode 0600). ``key_file`` names a file
instead.

The call goes out only where it can change the answer:

================== ======= ======================================
check              default skipped prompt
================== ======= ======================================
``min_words``      4       "yes", "keep going"
``min_candidates`` 2       fewer claims than a choice needs
``monthly_usd``    4.0     any, once the month's spend reaches it
================== ======= ======================================

A skipped prompt gets the lexical search and the phrase lists. With Jev
on, the hook never loads the cross-encoder, so its memory stays free.
A prompt past 2,000 characters goes out cut to that length.

A prompt with ten candidates takes about 900 input tokens. At $0.042 per
million input tokens, with output free, that arithmetic is a prompt cost
of $0.00004. The
hook records each call's tokens at ``usd_per_mtok_in``, or the API's own
figure when OpenRouter sends one, in ``$XDG_STATE_HOME/ljos/jev-cost.toml``.
For OpenRouter, set ``endpoint = "https://openrouter.ai/api/alpha/decisions"``
and ``model = "typesafe/jev-1.13"``.

Jev answers each question with a probability. ``bears_at`` (0.5) is the
one at which a claim goes in, and ``cue_at`` (0.5) the one at which a
prompt counts as a correction or a choice. Raise ``bears_at`` to 0.7 when
off-topic claims come through.

Jev keeps no cache of its own, so ljos keeps one. An identical request
within ``cache_days`` (7) is answered from ``$XDG_CACHE_HOME/ljos/jev`` at no
cost, and the doctor row counts those answers as cached. A panel asked
twice, a retried ballot and a repeated prompt with the same claims all
hit it. ``cache_days = 0`` turns it off.

Two or more judges can answer side by side. Each ``[judges.NAME]`` table
names a judge. ``[route]`` says which judges answer each decision. Here a chat
model on an compatible endpoint judges the prompt, Jev and that
model pool their ballots, and the chat model reviews due claims:

.. code:: toml

   enabled = true
   key_file = "~/.config/ljos/jev.key"     # the default judge: Jev

   [judges.chat]
   backend = "chat"
   endpoint = "https://llm.example.org/v1"
   model = "model-b"
   key_env = "LLM_API_KEY"
   budget_ms = 6000

   [route]
   prompt = ["chat"]
   ballot = ["default", "chat"]
   audit = ["chat"]
   review = ["chat"]

A decision with two or more judges is answered by their pool. A
probability is the weighted mean of the judges' log-odds. Two judges at
0.8 stay at 0.8, and judges at 0.9 and 0.1 meet at 0.5. A choice is
the normalised weighted geometric mean of their distributions, and a
score the weighted mean. ``weight`` in a judge's table moves its share. A
question no judge answered is missing from the pool, so the caller falls
back as it does on a partial reply. A caller with no judge at all takes
its local path on any of these: no file, ``enabled = false``, no key, an
error, or no answer inside ``budget_ms`` (2000). The log keeps every judge's answer
beside the pool, so each judge can be scored once outcomes are known.

These judges are fast and calibrated, and they do not reason. A
decision they leave open goes to a persona that does: a persona with a
runner reasons in a session of its own.

.. code:: console

   $ ljos persona buildengineer --anchor 0.25 --view "Runs production pipelines." --runner NAME
   $ ljos vote surf-ab12 --as buildengineer --jev
   buildengineer: Jev leaned A at confidence 0.61, under the 0.80 cut; not cast.
     buildengineer reasons in its own session in herdr pane w1:p2; then `ljos consensus surf-ab12`

The runner is any coding runner the machine runs: the terminal harness, the command harness, the build harness, omp, hermes or Antigravity's ``agy``.
Each runs on whatever model it is set to, and switching one to ``model-b`` on an compatible endpoint changes only the model it reasons with.
It is a ``[[harness]]`` in ``~/.config/ljos/harnesses.toml``.
Its ``start`` and ``resume`` argv say how it opens a session and how it resumes the latest one in a directory, for example the ``resume`` argv the harness table names.
It starts in a pane the person can watch and talk in: a herdr workspace when herdr's server is up, else a window of the tmux session ``ljos-personas``.
A ``[[tool]]`` the runners file declares is tried before either (see *Open persona panes in another tool*).
A runner that exits non-zero is resumed in its pane, at most three times a minute.
It works in the persona's home, ``$XDG_STATE_HOME/ljos/personas/NAME``, as the seat named after the persona.
Its memories, ballots and trust rows are therefore the persona's.
It reads the issue and the pack, writes its reasoning with ``ljos note``, and casts ``ljos vote --as NAME``.
Each runner continues the latest session of the directory it starts in, so the next hand-off resumes the same conversation, and an open pane takes the next task in place.
A task goes to the persona's inbox, and the pane is told the file in one line.
``ljos ask NAME "..."`` puts a question to the persona the same way.
``ljos supervise NAME --harness RUNNER --exit crash --at SECONDS`` says whether that child should be started again, under the restart policy (permanent, transient, temporary, three strikes in a minute), and prints the herdr line when the answer is yes.
It does not start the pane.
``ljos panel --jev`` hands each unsure persona's brief to its own session when the panel splits.
A persona with no runner gets a brief for a subagent, as before.

``ljos due --judge`` puts the due page to the ``review`` judges. Each claim
goes with the newer claims the pack keeps on the same subject. A claim
the pool puts at 0.9 or more is graded recalled. One at 0.1 or less
is printed as contradicted, for the agent to supersede or withdraw, and
stays due. Everything between stays due. No judge lapses a claim,
because a lapse says that a reader forgot it.

Put the judgments to a local model or a runner
==============================================

The same four judgments go to any judge that answers in Jev's shape.
``backend`` in ``jev.toml`` picks it; the parsers, the cache, the ledger and
the doctor row are shared, and every fallback above still applies.

A chat-completions endpoint, hosted or a local llama-server, takes one
JSON-mode chat completion per judgment set. ``endpoint`` is the base URL
and ``model`` the name the server knows. A local server needs no key;
``usd_per_mtok_in = 0`` keeps the ledger exact for one that is free.

.. code:: toml

   enabled = true
   backend = "chat"
   endpoint = "http://127.0.0.1:8089/v1"
   model = "model-c"
   usd_per_mtok_in = 0
   budget_ms = 6000

A runner on the machine is the judge through ``command``: an argv that
reads the request JSON on stdin (``state``, and ``questions``, each with a
``type`` of ``noul`` or ``choice``, ``instructions`` and ``criteria``) and prints
``{"answers": {...}}`` on stdout inside ``budget_ms``. A ``noul`` answer is
``{"noul": p}`` or a bare number; a ``choice`` answer is ``{"choice": KEY, "confidence": p, "probabilities": {...}}``, and a bare key means a
sure one. A question with no answer refuses the whole reply, so the hook
takes its local path rather than half a judgment.

.. code:: toml

   enabled = true
   backend = "command"
   command = ["/path/to/judge.sh"]
   budget_ms = 20000

The hook budget bounds the prompt path, so a harness that takes seconds
suits ballots and the stop audit more than the prompt hook; ``min_words``
and ``min_candidates`` keep the prompt calls few. The log carries the
``backend`` name, and ``ljos doctor`` prints it before the model.

Cast persona ballots through Jev
================================

With Jev on, a persona's ballot can come from Jev instead of a subagent.
The issue needs an ``Options: A, B`` line.
``ljos file "TITLE" --options A,B`` writes that line as the first line of the body.
``ljos options ISSUE A,B`` appends it to an issue that already exists, and refuses fewer than two.

.. code:: console

   $ ljos vote ljos-abcd --as security-reviewer --jev
   judge:jev-1.13.0: cast age for security-reviewer at confidence 0.96
   $ ljos panel ljos-abcd --jev
   3 personas on ljos-abcd through Jev: all sure, all age; one judge ballot cast

Jev reads the persona's brief and answers two choice questions: the
persona's vote, and what the rest of the panel will pick. The option
order is shuffled per persona. A cast ballot is recorded as
``judge:MODEL``, not as the persona, and ``--used`` names that judge. The
chosen option's probability is the ballot's confidence. The forecast is
the judge's prediction, and ``consensus`` does not read it. A note on
the issue says the ballot came from Jev. ``learn`` updates the trust row
whose ``to`` is that judge.

Jev's confidence is ``(K·p_max − 1)/(K − 1)``, the same cut on every
backend. Under ``escalate_below`` (0.8), ``vote --jev`` does not cast. It
notes Jev's lean. A persona with a runner gets the ballot in its own
session; for any other, the command names the brief to start a subagent
from.

A panel casts only when every seated persona is sure and all agree. An
all-one-model panel is one voter, so it casts one ballot. A split or
unsure panel casts nothing and writes a brief per seat for subagents.
The metered model then spends only on the contested questions. A ballot
costs about 2,000 input tokens, or $0.00008.

Every Jev answer goes to ``$XDG_STATE_HOME/ljos/jev-log.jsonl``, with the
claim ids, a hash of the prompt, the latency, a timeout or a failure,
and the backend and model. The file does not keep the prompt or the
claim text. ``ljos judge-score`` joins that log with outcomes and reports
accuracy and calibration per decision. ``ljos learn`` scores a cast
ballot's confidence as it does any voter's.
