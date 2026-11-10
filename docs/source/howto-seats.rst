:description: Add a seat to the tracker and grant it the right to close named tickets.
:cover: https://leidarljos.github.io/ljos/_static/og.png

===================================
Add a seat and let it close tickets
===================================


The tracker's ``vissue.toml`` lists the seats. ``ljos seat add NAME`` writes the row,
so nobody edits the file by hand.

.. code:: console

   $ ljos seat add carol

By default a seat cannot close a ticket. ``ljos finish`` leaves the ticket open, and a person closes it.
A grant changes that for one seat. Here a person says in their own words that carol may close
the tickets she filed or has claimed, and a recorder writes it down:

.. code:: console

   $ ljos seat grant carol close=own --quote "carol may close what she files" --at 2026-10-29T02:00:00Z --via alice

The row lives in ``vissue.toml``. It holds the quote, the time, the recorder named by ``--via`` and
the tracker commit. A grant edited into the file by hand is ignored. A seat cannot grant itself.
Every close that uses the grant cites the record.
