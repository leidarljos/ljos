:description: The row ljos doctor prints for a missing writer, seat, tracker, binary, Model Context Protocol entry or registry, and the command that fixes it.
:cover: https://leidarljos.github.io/ljos/_static/og.png

===================
Troubleshoot doctor
===================


``ljos doctor`` prints a summary line, then the rows that need action.
Each row is ``no``, ``warn`` or ``info``, then the name, then the state.
Exit 1 when a ``no`` row is a required part, the pack, the note queue, or a tracker root that is missing or not writable.
A ``warn`` row does not change the exit code.
These blocks copy the lines that command printed.
The home in them is ``/tmp/seat``, except the Model Context Protocol (MCP) blocks, which used ``/tmp/seat-probe``.
When the registry answers, a missing program's row names that version. These blocks do not pin one.

The pack writer
===============

A checkout that had only ``ljos`` and ``ljos-mcp`` on ``PATH`` printed this summary and these pack rows.
The other missing programs used the same shape, not on PATH, then ``cargo install --locked`` and the crate name. When the registry answers, the row also names that version.
Exit 1.

..
   # proseguard:off

.. code:: text

   $ ljos doctor
   ljos is not ready: ljos-policyd, vissue, deedar, claimdag, packset, packsetd and memory do not answer. Optional extras: `ljos doctor --all`.
   no  packset not on PATH, so run `cargo install --locked packset`.
   no  packsetd    not on PATH, so run `cargo install --locked packset`.
   no  memory    http://127.0.0.1:8761 does not answer. no pack writer is answering and packset is not on PATH, so run `cargo install --locked packset`.

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

Under a temporary home the writer does not start on its own.
The home was ``/tmp/seat``.
The printed row is:

..
   # proseguard:off

.. code:: text

   no  memory    http://127.0.0.1:8761 does not answer. the memory writer does not start on its own under a temporary HOME (/tmp/seat), so run `ljos doctor --fix`.

..
   # proseguard:on

No seat
=======

With no ``LJOS_SEAT``, no ``--seat``, and no seat record, the short doctor prints:

..
   # proseguard:off

.. code:: text

   warn    seat source no seat rule matched; reading every seat, so run `export LJOS_SEAT=NAME`.

..
   # proseguard:on

The row is a warning.
It does not by itself exit 1.
``ljos seat`` prints ``source`` as ``no seat rule matched; reading every seat`` and exits 0.

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
