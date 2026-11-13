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
A crates.io number is the version the registry returned for that run.

The pack writer
===============

A checkout that had only ``ljos`` and ``ljos-mcp`` on ``PATH`` printed this summary and these pack rows.
The other missing programs used the same shape, ``not on PATH; cargo install --locked`` and the crate name, with the crates.io version when the registry answered.
Exit 1.

..
   # proseguard:off

.. code:: text

   $ ljos doctor
   ljos is not ready: ljos-policyd, vissue, deedar, claimdag, packset, packsetd and memory do not answer. Optional extras: `ljos doctor --all`.
   no  packset not on PATH; cargo install --locked packset (crates.io 0.17.10)
   no  packsetd    not on PATH; cargo install --locked packset (crates.io 0.17.10)
   no  pack    http://127.0.0.1:8761 does not answer. no pack writer is answering, and packset is not on PATH. cargo install --locked packset

..
   # proseguard:on

The next command is ``cargo install --locked packset``.
That installs ``packset`` and ``packsetd``.

With ``packset`` on ``PATH`` and nothing listening, a home that is not a temporary directory printed:

..
   # proseguard:off

.. code:: text

   no  pack    http://127.0.0.1:8761 does not answer. no pack writer is answering on 127.0.0.1:8761. Run `packset ensure`

..
   # proseguard:on

Under a temporary home the writer does not start on its own.
The home was ``/tmp/seat``.
The printed row is:

..
   # proseguard:off

.. code:: text

   no  pack    http://127.0.0.1:8761 does not answer. the memory writer does not start on its own under a temporary HOME (/tmp/seat); run `packset ensure` to start it

..
   # proseguard:on

No seat
=======

With no ``LJOS_SEAT``, no ``--seat``, and no seat record, the short doctor prints:

..
   # proseguard:off

.. code:: text

   warn    seat source no seat rule matched; reading every seat. Set one with --seat NAME or export LJOS_SEAT=NAME

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

   info    tracker vissue failed (exit 1): not inside a tracker. Run `ljos onboard` to create the tracker

..
   # proseguard:on

``VISSUE_ROOT`` pointing at a directory that is not there fails the doctor.
The summary names the tracker, and the row is:

..
   # proseguard:off

.. code:: text

   no  tracker vissue 0.23.1 root=/tmp/seat/no-such-tracker prefix=Software from VISSUE_ROOT=/tmp/seat/no-such-tracker; root is not a directory. `mkdir -p /tmp/seat/no-such-tracker/Software` creates it

..
   # proseguard:on

A tracker directory this account cannot write fails the same way:

..
   # proseguard:off

.. code:: text

   no  tracker vissue 0.23.1 root=/tmp/seat/tracker prefix=Software from VISSUE_ROOT=/tmp/seat/tracker; /tmp/seat/tracker is not writable. `chmod u+w /tmp/seat/tracker` lets this account file tickets

..
   # proseguard:on

A stale config is a ``vissue/config.toml`` that names a missing directory.
The printed row is:

..
   # proseguard:off

.. code:: text

   no  tracker vissue 0.23.1 root=/tmp/seat/old-tracker prefix=Software from seat config or working directory; /tmp/seat/.config/vissue/config.toml names /tmp/seat/old-tracker, which is not a directory. `mkdir -p /tmp/seat/old-tracker/Software` creates it

..
   # proseguard:on

A missing habitat binary
========================

``deedar`` not on ``PATH``, with the registry answering, printed:

..
   # proseguard:off

.. code:: text

   no  deedar  not on PATH; cargo install --locked deedar-cli (crates.io 0.6.1)
   info    tracker vissue not on PATH; cargo install --locked vissue-cli
   no  claim graph claimdag not on PATH; cargo install --locked claimdag-cli

..
   # proseguard:on

``ljos doctor --all`` also prints the deed store row, which names the same install:

..
   # proseguard:off

.. code:: text

   info    deed store  deedar not on PATH; cargo install --locked deedar-cli

..
   # proseguard:on

The ``vissue`` and ``claimdag`` rows are the same shape, with ``vissue-cli`` and ``claimdag-cli``.
A required binary exits 1.
The tracker and deed store rows are info when the program is the thing that is missing.

Model context protocol registration
===================================

On a machine that is not a cloud VM, a missing command in ``~/.cursor/mcp.json`` printed:

..
   # proseguard:off

.. code:: text

   no  mcp ljos-mcp: /no/such/ljos-mcp does not exist (/tmp/seat-probe/.cursor/mcp.json), so the MCP client has no tools. Run `ljos onboard --harness cursor`.

..
   # proseguard:on

Exit 1.
A command that exists and is not this install's ``ljos-mcp`` is a warning. The exit stays 0 when nothing else failed:

..
   # proseguard:off

.. code:: text

   warn    mcp warning: ljos-mcp: /tmp/seat-probe/other/ljos-mcp is not this install's /agent/repos/ljos/target/debug/ljos-mcp (/tmp/seat-probe/.cursor/mcp.json). Run `ljos onboard --harness cursor`.

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

A missing binary then has no version in parentheses: ``not on PATH; cargo install --locked packset``.
The warning does not by itself exit 1.
A copy already in the day cache is still used, and the row says ``crates.io (cached)``.
