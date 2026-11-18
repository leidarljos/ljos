:description: Make the tracker and the deed store with a few shell lines, for a seat that does not run ljos onboard.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=======================================
Make a tracker and a deed store by hand
=======================================


``ljos onboard`` makes the tracker and the deed store, and it also wires a runner and installs hooks.
Make them by hand when you want only the two directories, or when a script sets up the seat.
``ljos file`` and ``ljos sitting`` need both.

The tracker
===========

The tracker is a directory with a ``vissue.toml`` and a ``Software/`` directory.
vissue finds it through ``VISSUE_ROOT``.

.. code:: bash

   mkdir -p "$HOME/tracker/Software"
   printf 'prefix = "Software"\n' > "$HOME/tracker/vissue.toml"
   git -C "$HOME/tracker" init -q
   export VISSUE_ROOT="$HOME/tracker"

The ``git init`` line is the one you may leave out.
With it, each write is a commit.
Without it, ``ljos file`` still files the ticket, and prints that the tracker is not a git checkout.
Put the ``export`` line in your shell profile, so every shell finds the tracker.

The deed store
==============

The deed store is a directory of signed records of finished work.
With ``DEEDAR_URL`` unset, deedar uses ``$XDG_DATA_HOME/deedar/store``, and ``$HOME/.local/share/deedar/store`` when ``XDG_DATA_HOME`` is unset.
deedar does not make that directory on its own, so ``ljos sitting`` says the deed store does not answer until someone has.
An empty directory is a store: the first deed writes its layout.

.. code:: bash

   mkdir -p -m 700 "${XDG_DATA_HOME:-$HOME/.local/share}/deedar/store"

A store somewhere else needs no directory here. Set ``DEEDAR_URL`` to its address instead.

Check
=====

``ljos doctor`` stops printing the ``tickets`` and ``deeds`` rows once both exist.
Filing a ticket proves the tracker:

.. code:: console

   $ ljos file "First ticket" -p demo

The command prints the new ticket id.
``ljos sitting ID`` then opens a sitting on it.
