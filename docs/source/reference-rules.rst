:description: User rules, what a rule reads, the checks ljos keeps for the tracker, and the threat model of the pack token.
:cover: https://leidarljos.github.io/ljos/_static/og.png

================
Rules and checks
================


User rules
==========

``ljos prefer --user TEXT --why REASON`` writes one user rule.
The file is ``$XDG_CONFIG_HOME/ljos/user-rules.json``, or ``~/.config/ljos/user-rules.json`` when that directory is unset.
Every seat on this machine reads it. The row does not decay and it is not reviewed, and the command does not also store a seat preference.
``--why`` is required with ``--user``.
``--promote ID`` copies that lesson into a user rule and leaves the lesson in the pack.
A promoted rule is stated ahead of a prompt only when the pack answers for that lesson within 400 ms, under the same id, stored as the person's; a pack that is slow, down or answers otherwise leaves it out of the prompt.
The row keeps a hash of the lesson's kind, text, ``ts``, ``valid_from`` and stored origin, and a record under that id that hashes otherwise, such as an id retracted and posted again, is another lesson; a rule promoted before the hash was kept is not stated until it is promoted again.
The read goes only with this seat's pack token, and only after the same read without it was refused, so a listener that answers without the token, or a packsetd run with ``PACKSET_AUTH=off``, confirms no promoted rule.
``ljos rules`` lists a promoted rule the pack does not confirm apart from the user rules, as ``unverified`` with the reason.
The hook still enforces such a rule's pattern and its mention and reply denials, and the denial says the rule is unverified and why.
``--forget ID`` leaves a tombstone so a later sync does not bring the row back.
``--sync`` pulls and pushes user rules through the tracker repository. Nothing leaves the machine unless that repository's ``.ljos/sync.toml`` names a scope. Then each machine's rules are sealed with age to the scope's recipients as ``.ljos/user-rules/<host>.json.age``, and a plaintext ``<host>.json`` this machine wrote before is removed. Plaintext files other hosts left are still read. A write and the session start run the same pass and follow ``LJOS_TRACKER_GIT``; ``--sync`` always pushes.
``--deny-mention HANDLE``, ``--deny-name NAME``, ``--deny-reply-to HANDLE`` and ``--deny-reply-name NAME`` may be repeated.
``ljos rule --user PATTERN --verdict deny|ask --why TEXT`` writes the argv law into the same file instead of the seat pack.
Without ``--user``, both commands keep writing the seat pack.

What a rule reads
=================

A rule is tried on every command a line runs, the first and the rest. The
line is split on ``&&``, ``||``, ``;``, ``|``, ``&`` and new lines
outside quotes, and each piece is read again inside: the bodies of
``$(...)``, backticks, ``<(...)``, ``>(...)`` and subshells; the script of
``sh -c``, ``eval``, ``env -S``, ``trap``, ``alias``, ``su -c``, ``script -c``,
``flock -c`` and ``nix-shell --run``; a here-string or here-document fed to a
shell, and ``echo`` or ``printf`` piped into one; the commands of ``find -exec``; and the line ``ssh`` runs on its host. Leading ``NAME=value`` words,
wrappers (``sudo``, ``doas``, ``env``, ``time``, ``nohup``, ``exec``, ``command``,
``nice``, ``ionice``, ``timeout``, ``setsid``, ``stdbuf``, ``xargs``, ``flock``,
``chronic``, ``unbuffer``, ``watch``), ``case`` patterns and function
definitions come off, and quotes, ``$'...'`` and escapes come off each
word, so ``git 'push'``, ``$'git' push`` and ``git -c alias.p=push p`` read as
``git push``. A line nested more than eight deep is warned on whole.

A rule reads the line as written. It does not see a command whose name
comes from a variable (``$g push``), a script run from a file, an alias or
git alias defined in an earlier call, a recipe in a Makefile or justfile,
or a call made from inside another language (``python -c``, ``perl -e``). A
rule warns an agent off its mistakes; it is not a sandbox. For a push that
must never happen, protect the branch on the forge.

Checks ljos keeps
=================

policyd answers every command with one of three answers. ljos keeps two checks of its own,
because only ljos knows the tracker and the sitting. No answer stops a command: ljos never
refuses, asks or holds one. A cloud seat's push rule, below, is the one exception.

================================== ===========================================================================
Answer                             What happens
================================== ===========================================================================
``allow``                          the command runs
``allow``, ``warn``, rule, warning the command runs, and the hook shows one line with the rule and its warning
``rewrite``                        the command runs in a fixed form
================================== ===========================================================================

An older policyd that answers ``deny`` or ``prompt`` reads as ``allow`` with that rule's warning. The rule's
verdict stays a label: ``ljos rules`` lists it, the book log and ``ljos digest`` count it, and a hook shows
it as :literal:`warning: REASON (ljos rule \`PATTERN\`)`. A warning is the only thing between a runner and a
destructive command. For a command that must never run, protect the branch on the forge, keep backups,
and keep the runner where it cannot reach what it must not touch.

On the wire, a rewrite is one line of three tab-separated fields: ``allow``, ``rewrite`` and the new command.
The new line runs only when the guard has no warning on it. The hook hands it back as the updated input for
the harnesses that accept one, including the editor harness's ``preToolUse``. A runner without that field runs the old line, with
a warning that names the new command. The first rewrite runs the command through ``uv run``.

Each swap also shows a note naming both commands and leaves a row in
``$XDG_DATA_HOME/ljos/rewrite.jsonl``. Set ``rewrite = false`` in a repository's
``ljos.toml`` to run its lines as written. The log keeps a row for a swap it declined too.

If ``ljos-policyd`` is missing, slow or crashes, the command runs and ljos logs a warning.
Nothing in ljos turns a fault into a refusal.

By default a warning is ``allow``, the rule and one warning, and the command runs.
block-on-warning does not block.
ljos asks ``ljos-policyd check --config PATH -- COMMAND`` when ``policyd_config = "PATH"`` is in the user's or the seat's ``ljos.toml``, at the top or under ``[gates]``; the seat's file wins, ``""`` names none, and a relative path is from that file's directory.
A repository's ``ljos.toml``, ``.ljos/gates.toml`` and ``LJOS_CONFIG`` do not name it.
A config with ``block-on-warning true`` does not block. ljos warns with the rule's label, and the command runs.
The hook does not deny the tool call, ``ljos policy --fail-on-deny`` does not exit 1 for that warning, and a guard shim does not stop the command.
A static, seat, user or pack rule still warns.
Without a named config, a ``deny`` or ``prompt`` from policyd reads as its warning, and the command runs.
A check that fails open still allows.
A named config that is not there fails open on every command, with no rule or warning.
:doc:`How-to <howto>` has the steps.

A command that deletes or moves a tree holding the person's key directory, such as ``rm -rf /``, ``rm -rf ~`` or ``shutil.rmtree(os.path.expanduser('~'))``, runs with ljos's own label ``uproots-person-key``.
Its warning names the tree and the key directory, and the bypass log keeps it as a ``warn`` row.
A config with ``block-on-warning true`` warns with a label where policyd warns on the same command. It does not block.
The same goes for a recursive delete outside ``/tmp``.
A command that changes the key directory, a file in it or the directory above it runs with ``changes-person-key``.
``changes-kept-prompt`` labels a change to the runtime directory where the hook keeps the person's latest prompt, or to a file in it.
One that writes any other file of the seat, an ljos binary, a hook or the file that registers one or the ljos config, runs with ``writes-seat-file``.
Each warning names the word the command changes.
A tool call's ``ljos prefer --review --keep`` keeps the rule as the agent's and names nothing.
Under ``setsid``, ``script`` or ``env -i``, as if from the person's terminal, the call names ``prefer-keep-faked-terminal``.
The rule is still the agent's.

The tracker write check warns on a command, or a file tool, that writes the tracker's files by hand. Ticket text is data.
A ticket body that names a path or a command is never read as one.

The push check warns on a push onto a protected branch when the ticket of
the current sitting has no settled consensus. It covers ``develop`` and ``main`` unless
``consensus_branches`` in ``ljos.toml`` names others. With one active seat in ``vissue.toml``, that seat's own vote
settles it. This is solo mode. With two or more seats, the normal quorum applies.
The ``pre-push`` git hook, once installed, still holds that push under a runner.

Every other push rule is policyd's. Each runs the push.

====================================================================================== ===============================
Push                                                                                   Answer
====================================================================================== ===============================
an ordinary push of a branch, a tag, or a lease push to a branch that is not protected runs
a lease push or a force push onto a protected branch                                   runs, named ``git-force-push``
deleting a remote branch                                                               runs, named ``git-push-delete``
a push onto a protected branch, with no settled consensus on the sitting's ticket      warns
rewriting published history                                                            warns, by policyd's table
====================================================================================== ===============================

A cloud seat has one push rule. It may push the branches named in its ``ljos.toml`` and ``develop``.
A cloud seat cannot force push, delete a branch, or push to a branch outside its allowlist.
Every other push from a cloud seat runs with a warning.
Opening or merging a pull request is not refused. Branch protection on the remote does the rest.

The remote is the one the command names, else the branch's upstream,
else ``origin``, read in the directory a ``cd`` or ``git -C`` before the push
names.

Threat model
============

The pack token is sent only to the socket address the writer's URL resolved to, and only after that address has refused an unknown atom id with no token and with a token that is not this seat's. The request that carries the token may be the next connection; a later request proves again. A redirect is not followed, and a proxy in the environment is not used.
A process of the same user that reads the token file can still answer as the pack. The proof cannot tell that process from packsetd; a writer that signs its answers, or that runs as another user, would.
