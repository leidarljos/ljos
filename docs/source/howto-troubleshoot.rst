:description: The row ljos doctor prints for a missing writer, seat, tracker, binary, Model Context Protocol entry or registry, and the command that fixes it.
:cover: https://leidarljos.github.io/ljos/_static/og.png

===================
Troubleshoot doctor
===================


``ljos doctor`` prints the rows that need action, then a summary line.
Each row is ``no``, ``warn`` or ``info``, then the name, then the state.
Exit 1 when a ``no`` row is a required part, the pack, the note queue, or a tracker root that is missing or not writable.
A ``warn`` row does not change the exit code.
These blocks copy the lines that command printed.
The home in them is ``/tmp/seat``, except the Model Context Protocol (MCP) blocks, which used ``/tmp/seat-probe``.
When the registry answers, a missing program's row names that version. These blocks do not pin one.

The pack writer
===============

A checkout that had only ``ljos`` and ``ljos-mcp`` on ``PATH`` printed these pack rows and this summary.
The other missing programs used the same shape, not on PATH, then ``cargo install --locked`` and the crate name. When the registry answers, the row also names that version.
Exit 1.

..
   # proseguard:off

.. code:: text

   $ ljos doctor
   no  packset not on PATH, so run `cargo install --locked packset`.
   no  packsetd    not on PATH, so run `cargo install --locked packset`.
   no  memory    http://127.0.0.1:8761 does not answer. no pack writer is answering and packset is not on PATH, so run `cargo install --locked packset`.
   ljos is not ready: ljos-policyd, vissue, deedar, claimdag, packset, packsetd and memory do not answer. Optional extras: `ljos doctor --all`.

..
   # proseguard:on

The next command is ``cargo install --locked packset``.
That installs ``packset`` and ``packsetd``.

With ``packset`` on ``PATH`` and nothing listening, a home that is not a temporary directory printed:

..
   # proseguard:off

.. code:: text

   no  memory    http://127.0.0.1:8761 does not answer. no pack writer is answering on 127.0.0.1:8761, so run `ljos doctor --fix`.

..
   # proseguard:on

That one command starts the writer. ``ljos doctor --fix`` runs ``packset ensure`` itself, and prints one line that says it did.

Under a temporary home memory and the command guard are off, and ``ljos doctor --fix`` does not start a writer.
The home was ``/tmp/seat``.
The printed row is one line, and ``ljos search`` and ``ljos consensus`` print that same line and exit 1:

..
   # proseguard:off

.. code:: text

   no  memory    memory and the command guard are off because the home is under /tmp (/tmp/seat); set HOME to a directory outside /tmp

..
   # proseguard:on

The first-run tests in ``crates/ljos-cli/tests/first_run.rs`` build a seat home, so they hit this row when the seat sits in a temporary directory.
A checkout under ``/tmp`` or ``/var/tmp`` used to put the seat next to it, and the three tests that run a whole seat failed on that row.
The seat now goes beside the checkout when that place is outside every temporary root and has no git checkout above it.
Otherwise it goes under ``~/.cache/ljos-test-seats``.
When neither place fits, the test prints one line that says so and stops, or fails with that line when ``LJOS_TEST_REQUIRE_TOOLS=1``.

A ``packsetd`` already listening on the asked port for another home, or with another token, is not this seat's writer.
A writer whose pack home is the directory ``PACKSET_HOME`` names is this seat's, even when its process home is that directory.
``/health`` can still answer, and a later ``ljos send`` or ``ljos consensus`` is refused.
The memory row is ``no`` and ``ljos doctor`` exits 1.
The row names ``set PACKSET_PORT`` for another home, or ``packset stop && packset ensure`` for another token.

No seat
=======

With no ``LJOS_SEAT``, no ``--seat``, and no seat record, the short doctor prints:

..
   # proseguard:off

.. code:: text

   warn    seat source no seat is set; reads use every seat, so run `export LJOS_SEAT=NAME`.

..
   # proseguard:on

The row is a warning.
It does not by itself exit 1.
``ljos seat`` prints ``seat`` and ``holder`` as ``none``, names ``LJOS_SEAT`` and exits 0.

The tracker
===========

With ``vissue`` installed and no tracker configured, the row is info and this failure does not by itself exit 1:

..
   # proseguard:off

.. code:: text

   info    tickets no tracker at /tmp/seat/.local/share/vissue/tracker, so run `ljos onboard`.

..
   # proseguard:on

The row names the directory ``ljos onboard`` makes, the way the deeds row names its store.
It does not print vissue's refusal, which names the directory you were in.
Ticket reads such as ``ljos show ID`` print the same one-line ``ljos onboard`` fix when no tracker is configured.

``VISSUE_ROOT`` pointing at a directory that is not there fails the doctor.
The summary names the tracker, and the row is:

..
   # proseguard:off

.. code:: text

   no  tickets vissue 0.24.1 root=/tmp/seat/no-such-tracker prefix=Software from VISSUE_ROOT=/tmp/seat/no-such-tracker; root is not a directory. `mkdir -p /tmp/seat/no-such-tracker/Software` creates it

..
   # proseguard:on

A tracker directory this account cannot write fails the same way:

..
   # proseguard:off

.. code:: text

   no  tickets vissue 0.24.1 root=/tmp/seat/tracker prefix=Software from VISSUE_ROOT=/tmp/seat/tracker; /tmp/seat/tracker is not writable. `chmod u+w /tmp/seat/tracker` lets this account file tickets

..
   # proseguard:on

A stale config is a ``vissue/config.toml`` that names a missing directory.
The printed row is:

..
   # proseguard:off

.. code:: text

   no  tickets vissue 0.24.1 root=/tmp/seat/old-tracker prefix=Software from seat config or working directory; /tmp/seat/.config/vissue/config.toml names /tmp/seat/old-tracker, which is not a directory. `mkdir -p /tmp/seat/old-tracker/Software` creates it

..
   # proseguard:on

A ``vissue`` killed between its write and its rename leaves a copy such as ``.issues.org.tmp.4242-1763600000-0`` beside the file.
The next logbook write, which has the file's issues lock, removes such a copy once it is a minute old.
The process id in the name decides nothing: it may be another host's, or a later process may have it now.

A git killed while it held the index leaves ``.git/index.lock``, and every commit in that checkout fails on it.
While its git runs, each ljos tracker commit and sync commit records its host name, boot id, process id and process start time in ``.git/ljos-index.owner``, with the device and inode of each ``index.lock`` made then and a mark of what that file last was: its size, its modify and change times, its inode generation where the file system gives one, and a SHA-256 of its bytes.
A later tracker or sync commit removes a lock a minute old, prints one line, and commits, when that record names this host and boot, the lock's device and inode and an ljos process that no longer runs, and the lock, read through one open descriptor, matches that mark in every field.
No live process may have the lock open, and no git may name the repository by its working directory, ``-C``, ``--git-dir``, ``--work-tree``, ``GIT_DIR``, ``GIT_WORK_TREE``, ``GIT_INDEX_FILE`` or ``GIT_COMMON_DIR``, each compared by device and inode through that process's own links, so a bind mount or another mount namespace does not hide it.
The lock is renamed aside in ``.git`` and removed only when the renamed file is still the one checked, unchanged, and that scan still finds no owner; otherwise it is put back:

.. code:: text

   tracker git: removed /tmp/seat/tracker/.git/index.lock, 120s old, which ljos process 4242 on box made and which no longer runs

A git that ljos stops at its time limit, or one killed some other way, leaves its lock, and the record stays beside it with that lock's last mark.
The lock is left while that ljos process runs, and a commit after the process has exited removes it.

A lock is named with the ``rm`` command and the commit is refused when there is no record, since it may be a git's on another host that shares the checkout; when the recorded process still runs; when the file is not what the mark says, since it may be a later file on the same inode; and when it is empty on a file system that gives no inode generation, since a later empty file would match.
A younger lock, or one a live process may own, is left and the write defers.
So is any lock when this process cannot see every process: a ``/proc`` mounted with ``hidepid``, a pid namespace other than the host's first, as in most containers, or a process of the lock owner's account whose open files, name, directory or environment cannot be read, such as a git that is not dumpable or an ``ssh-agent`` or ``gpg-agent``.
That a process has the lock open never clears it, since git keeps its lock by its name after it closes the file.

A claim the tracker did not answer for is not a claim, whether its wait ran out or a tool call's bound stopped ``vissue`` with ``vissue timed out``.
``ljos claim`` exits 1 and names ``ljos claim ID`` again, and a claim a note retries stays pending until the tracker records it.

A push replays the remote's commits under the commit lock and the issues lock of every board, a board only the remote has included.
When one of those locks cannot be taken, the push stops before the checkout changes, and a replay that fails is undone before the locks are let go:

.. code:: text

   tracker git: committed chore(issues): Software-t001 noted by saddle; push queued: push: /tmp/seat/tracker/Software/Software/issues.org.lock was not locked (it could not be opened, or another writer kept it past the wait), so the checkout was not rebased; the next push tries again

A ``git ls-files`` or ``git ls-tree`` that cannot list the boards stops the push the same way, before any board is locked or rewritten.

A missing habitat binary
========================

``deedar`` not on ``PATH``, with the registry answering, printed:

..
   # proseguard:off

.. code:: text

   no  deedar  not on PATH, and crates.io lists 0.6.1, so run `cargo install --locked deedar-cli`.
   info    tickets vissue not on PATH, so run `cargo install --locked vissue-cli`.
   no  claim graph claimdag not on PATH, so run `cargo install --locked claimdag-cli`.

..
   # proseguard:on

``ljos doctor --all`` also prints the deeds row, which names the same install:

..
   # proseguard:off

.. code:: text

   info    deeds  deedar not on PATH, so run `cargo install --locked deedar-cli`.

..
   # proseguard:on

The ``vissue`` and ``claimdag`` rows are the same shape, with ``vissue-cli`` and ``claimdag-cli``.
A required binary exits 1.
The tickets and deeds rows are info when the program is the thing that is missing.

Model context protocol registration
===================================

On a machine that is not a cloud VM, a missing command in ``~/.cursor/mcp.json`` printed:

..
   # proseguard:off

.. code:: text

   no  mcp ljos-mcp: /no/such/ljos-mcp does not exist (/tmp/seat-probe/.cursor/mcp.json) and the MCP client has no tools, so run `ljos onboard --harness cursor`.

..
   # proseguard:on

Exit 1.
A command that exists and is not this install's ``ljos-mcp`` is a warning. The exit stays 0 when nothing else failed:

..
   # proseguard:off

.. code:: text

   warn    mcp warning: ljos-mcp: /tmp/seat-probe/other/ljos-mcp is not this install's /home/you/.cargo/bin/ljos-mcp (/tmp/seat-probe/.cursor/mcp.json), so run `ljos onboard --harness cursor`.

..
   # proseguard:on

On a cloud VM, a codespace, or CI the same rows say to run ``ljos cloud up`` instead of ``ljos onboard --harness cursor``.

Secret variables
================

``ljos doctor`` and ``ljos_doctor`` name a secret variable and say whether it is set. They do not print its value. ``LJOS_CLOUD_AGE_SECRET`` names another variable, and a token pasted into it can read like a name, so the ``cloud age`` row says the variable is unset without repeating the name:

..
   # proseguard:off

.. code:: text

   no  cloud age   LJOS_CLOUD_AGE_SECRET names a variable that is unset, and /home/you/.config/ljos/cloud.age is missing

..
   # proseguard:on

A value that is not an age identity, or an age identity pasted into the setting, is named by the problem and never printed. The ``signing key`` row prints ``DEEDAR_HOST_SIGNING_KEY`` only when its value is a path:

..
   # proseguard:off

.. code:: text

   no  signing key DEEDAR_HOST_SIGNING_KEY is set, but its value is not the path of a key file; set it to that path, or to off for no signing

..
   # proseguard:on

The config rows show ``set`` for ``LJOS_TRACKER_TOKEN``, ``LJOS_WAKE_LAUNCHER_KEY`` and any other variable whose name has ``TOKEN``, ``SECRET`` or ``PASSPHRASE`` in it or ends in ``_KEY``. ``LJOS_SEAT_KEY`` shows its value when the value is a path, and ``set`` otherwise.

The registry
============

With ``curl`` unable to reach crates.io, doctor printed:

..
   # proseguard:off

.. code:: text

   warn    crates.io   did not answer, so the version check was skipped

..
   # proseguard:on

A missing binary then has no version in parentheses: not on PATH, so run ``cargo install --locked packset``.
The warning does not by itself exit 1.
A copy already in the day cache is still used, and the row says ``crates.io (cached)``.

Mistyped ticket id
==================

When a ticket id is close to one in the tracker, ``show``, ``note`` and
``finish`` print one line with the closest id and a command to run:

.. code:: text

   issue demo-kln not found, and the closest is demo-kiln, so run `ljos show demo-kiln`.

A sentence where the id goes is refused by name, and nothing is noted:

.. code:: text

   note: "the claim path rereads the file" is not a ticket id, so nothing was noted; run `ljos note ID TEXT`, or quote the whole note as one argument.

File without a project
======================

``ljos file`` needs a project when there is no sitting and no seat default.
The refusal names the exact command shape:

.. code:: text

   no project: name one with -p NAME, so run `ljos file "TITLE" -p NAME`.

Corrupt tracker config
======================

When ``vissue.toml`` is damaged, commands that read tickets stop with one
line and a fix command:

.. code:: text

   the tracker config does not parse, so run `ljos doctor`.

Empty search query
==================

``ljos search`` with no query prints one usage line with the exact
command shape, even when old queued lessons exist:

.. code:: text

   search: the query is empty, so run `ljos search "QUERY"`.

Free text on recall
===================

``ljos recall`` takes one ticket id, including a 32-hex tracker id.
Text with spaces, or a word that is not an id, exits 2 and points at the search command:

.. code:: text

   recall: "usability first-run ljos cli" is not a ticket id, so run `ljos search "usability first-run ljos cli"`.

Words typed without quotes are several arguments, and recall takes one id.
The line says so and prints the search that runs:

.. code:: text

   $ ljos recall some words
   ljos: recall takes one ticket id, so run `ljos search "some words"` to look up words.

``ljos search`` takes the same words without quotes as one query, beside ``-p``, ``-n`` and its other flags, so ``ljos search lexical default`` and ``ljos search "lexical default"`` are the same search.

A word that is not a command
============================

``lessons`` is a natural guess, and it is not a command.
The line exits 2 and names the three verbs that work with lessons, each as a command that runs:

.. code:: text

   ljos: no command lessons. To find lessons run `ljos search "lexical default"`, to save one run `ljos remember "The lexical default is BM25+."`, and to review the ones waiting run `ljos proposals`.

Words typed after it become the query, so ``ljos lessons kiln`` names ``ljos search "kiln"``.
Any other word that is near a verb names that verb.
A name with an ending, the start of one verb and a name with a verb as a part are all near:

.. code:: text

   ljos: no command prop, and the closest is proposals, so run `ljos proposals`.

A start that two verbs share, such as ``mem``, is not guessed, and the line names ``ljos help``.
