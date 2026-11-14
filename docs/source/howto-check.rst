:description: Read what ljos doctor prints, which rows fail, and when it exits 1.
:cover: https://leidarljos.github.io/ljos/_static/og.png

==============
Check the seat
==============


``ljos doctor`` prints whether the seat is ready and the rows that need action, and exits 1 when a required part does not answer, the note queue is damaged, or the tracker root is missing or not writable.
``ljos doctor --all`` prints every row.

.. [[[cog import ljos_version as v; v.lines("", "The block is the ``ljos`` row from that command on a scratch seat, taken from ljos {version}.", "")]]]

The block is the ``ljos`` row from that command on a scratch seat, taken from ljos 0.43.0.

.. [[[end]]]

.. [[[cog import ljos_version as v; v.lines("", "``ljos --version`` printed ``ljos {version}``.", "")]]]

``ljos --version`` printed ``ljos 0.43.0``.

.. [[[end]]]

The scratch seat's day cache already holds that version, so the row says ``crates.io (cached)`` and neither ``behind`` nor ``ahead of``.
The path stands in for the binary.
The scratch ``PATH`` holds only this ``ljos``, so the sample does not show the other seat binaries.
A release newer than the crates.io copy says ``ahead of crates.io`` on that program's row.
That is the release you installed. Search without the encoder is the ``encoder`` row:
``not installed. Search matches words``. A loaded encoder says ``loaded``. One that is still starting says ``loading`` and about how long to wait. One whose last encode failed says ``failed`` and the reason. The ``packset-embed`` row names ``sh scripts/install.sh``
without ``--no-embed``, or ``cargo binstall``. ``ljos statusline`` works without ``ljos-hud``.
``ljos doctor`` with no ``--all`` also prints the sitting gate. Under a runner it
names the seat and what a push onto a protected branch waits for. With no
runner above the shell it says the person's git is not gated.
``info`` rows, including a missing cloud guard, are optional. A cloud seat runs
``ljos cloud up`` for that row. A machine that is not one can leave it.

.. BEGIN ljos-sample doctor

.. code:: text

   $ ljos --version
   ljos 0.43.0
   $ ljos doctor --all
   ok  ljos    /path/to/ljos  0.43.0  crates.io (cached) 0.43.0

.. END ljos-sample doctor

Exit 1 when a required part does not answer, the note queue is damaged, or the tracker root is missing or not writable.
A tracker that has not been created is an info row and names ``ljos onboard``.
The deed store is reported and does not by itself exit 1.
The tracker row names how many commits origin lacks, and fails when that count
sits through the push wait. It also fails when another remote of the
tracker has a different head of the branch, as of the last fetch: two
seats pushing to two remotes each read only their own writes. A sitting's own tracker push goes to
every remote that carries the branch. A missing
claim graph is reported and is not a failure: the first claim creates it.
A seat nobody named is a warning. The ``seat source`` row says to set one with ``--seat NAME`` or ``export LJOS_SEAT=NAME``. It does not by itself exit 1.
A damaged note queue is a ``no`` row and fails: it holds lessons the pack has not taken.
An empty or missing log of either is whole.
The rows for a missing writer, seat, tracker, binary, Model Context Protocol entry and a registry that did not answer are on :doc:`Troubleshoot doctor <howto-troubleshoot>`.

Repair what doctor can
======================

``ljos doctor --fix`` repairs the problems that are local and reversible, then prints the doctor again.
Each repair is one line.
Each problem it leaves is one line, and that line names the one command to run.
A row doctor can repair ends with ``run ljos doctor --fix``.

It creates a missing ljos home (``$XDG_STATE_HOME/ljos``, or ``~/.local/state/ljos``) and a ``ljos.toml`` that sets no keys, so the defaults stand.
It renames a stale lock or pid file whose process is gone, adding ``.bak`` to the name.
It starts a pack writer that is not answering with ``packset ensure`` when ``packset`` is on ``PATH``.
It records the session seat when a runner session id is set and that seat is not recorded yet.

It does not change git hooks, runner hooks, policy, credentials or keys, and it does not delete a file.
A skill edited by hand, an env file ljos did not write, and a cloud guard file that is missing or not a known older copy are left in place, as before.
