:description: Sign a handover bundle and receive from the same sender again.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=========================
Hand work to another seat
=========================


Sign what you hand over
=======================

``ljos onboard`` writes ``~/.config/deedar/host.key`` when that file is missing.
Leave it in place. Replacing it makes ``ljos doctor`` say the key is not a
signer. Create one only when the file is absent:

.. code:: console

   $ test -f ~/.config/deedar/host.key || { head -c 32 /dev/urandom > ~/.config/deedar/host.key && chmod 600 ~/.config/deedar/host.key; }
   $ export DEEDAR_HOST_SIGNING_KEY=~/.config/deedar/host.key
   $ ljos doctor --all | grep 'host key'
   ok  signing key    /home/you/.config/deedar/host.key, the signing key for deeds

``handover`` then signs the manifest and the log head. Write the bag, then
the other seat receives it:

.. code:: console

   $ ljos handover --out /tmp/bag --issue proj-1a2b
   $ ljos receive /tmp/bag --keep ~/.local/share/ljos/peer.head

``proj-1a2b`` is an id this seat filed. A receiver adds
``signer = ed25519:<hex>`` to their deed store's ``layout`` and ``receive``
reports ``(accepted)``. Two seats share one ticket by this walk, in
sequence: A sits and hands over, B imports and sits after A releases,
both vote, consensus, finish closes. ``scripts/smoke.sh`` runs it on
scratch stores; ``scripts/herd.sh`` is the concurrent contention check.

Receive from the same sender again
==================================

.. code:: console

   $ ljos receive /tmp/bag --keep ~/.local/share/ljos/peer.head      # the first time
   ...
   3 deeds proven against a log of 34 entries, root 9c1e...
   kept this head in /home/you/.local/share/ljos/peer.head; pass it to --since next time
   $ ljos handover --out /tmp/bag2 --issue proj-1a2b --since-size 34  # the sender, next time
   $ ljos receive /tmp/bag2 --since ~/.local/share/ljos/peer.head --keep ~/.local/share/ljos/peer.head

``--keep`` writes the head of the log the bag carried. Tell the sender its
``size``; ``handover --since-size`` puts the bridge from that size into the
bag. ``--since`` then needs the bridge to start at the head you kept and end
at the one in the bag, so a sender who rewrote the log in between fails the
check. A bridge alone proves nothing, because the sender picks both of its
ends. This needs deedar with ``check --keep``.
